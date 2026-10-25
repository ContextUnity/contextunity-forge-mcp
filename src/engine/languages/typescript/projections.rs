use super::*;
use crate::core::semantic::{SourcePosition, StaticString, ValueExpr, ValueFlowFacts};
use crate::core::typed_facts::FlowStore;
use manifests::projections::{Api, Slot};

use manifests::semantic::{
    api_proven, lexical_scopes, unique_initializer, unshadowed, ProjectionPayload,
};

struct ProjectionCandidate {
    owner: String,
    rule: usize,
    position: SourcePosition,
    callee: String,
    static_call: bool,
    payload: ProjectionPayload,
}

thread_local! {
    static PROJECTIONS: std::cell::RefCell<Vec<ProjectionCandidate>> = const { std::cell::RefCell::new(Vec::new()) };
    static SETUP_SUMMARIES: std::cell::RefCell<HashMap<usize, Vec<crate::core::semantic::ObjectMember>>> = std::cell::RefCell::new(HashMap::new());
}

/// Transfer only the completed direct callback summary, before its enclosing call exits.
pub(super) fn complete_setup(
    function: Syntax<'_>,
    returned: Syntax<'_>,
    source: &str,
    definitions: &hashbrown::HashMap<&str, Syntax<'_>>,
) {
    use crate::core::semantic::{ObjectMember, ObjectMemberTarget};
    if function
        .parent()
        .is_none_or(|parent| parent.kind() != "arguments")
        || returned.kind() != "object"
        || returned.named_child_count() > 128
    {
        return;
    }
    let mut members = Vec::new();
    let mut names = HashSet::new();
    for item in returned.named_children(&mut returned.walk()) {
        if item.kind() == "comment" {
            continue;
        }
        let (name, value) = match item.kind() {
            "shorthand_property_identifier" => (text(item, source), item),
            "pair" => {
                let (Some(key), Some(value)) = (
                    item.child_by_field_name("key"),
                    item.child_by_field_name("value"),
                ) else {
                    return;
                };
                if !matches!(key.kind(), "identifier" | "property_identifier") {
                    return;
                }
                (text(key, source), value)
            }
            _ => return,
        };
        if !names.insert(name) {
            return;
        }
        let declaration = if matches!(value.kind(), "identifier" | "shorthand_property_identifier")
        {
            definitions.get(text(value, source)).copied()
        } else if matches!(value.kind(), "arrow_function" | "function_expression") {
            Some(value)
        } else {
            None
        };
        let Some(declaration) = declaration.filter(|node| {
            matches!(
                node.kind(),
                "function_declaration" | "arrow_function" | "function_expression"
            )
        }) else {
            continue;
        };
        let declared = declaration
            .child_by_field_name("name")
            .map(|node| text(node, source))
            .or_else(|| {
                declaration
                    .parent()
                    .filter(|node| node.kind() == "variable_declarator")
                    .and_then(|node| node.child_by_field_name("name"))
                    .map(|node| text(node, source))
            })
            .unwrap_or(name);
        members.push(ObjectMember {
            name: name.to_owned(),
            position: SourcePosition {
                line: declaration.start_position().row + 1,
                column: declaration.start_position().column,
            },
            target: if declared == name {
                ObjectMemberTarget::SameName
            } else {
                ObjectMemberTarget::Alias {
                    name: declared.to_owned(),
                }
            },
        });
    }
    if !members.is_empty() {
        SETUP_SUMMARIES.with(|summaries| {
            summaries.borrow_mut().insert(function.id(), members);
        });
    }
}

fn static_path(node: Syntax<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, source).to_owned()),
        "member_expression" if node.child_by_field_name("optional_chain").is_none() => {
            let object = static_path(node.child_by_field_name("object")?, source)?;
            let member = node.child_by_field_name("property")?;
            (member.kind() == "property_identifier")
                .then(|| format!("{object}.{}", text(member, source)))
        }
        _ => None,
    }
}

fn argument_values(call: Syntax<'_>) -> Option<Vec<Syntax<'_>>> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let values: Vec<_> = arguments
        .named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect();
    if values.iter().any(|n| n.kind() == "spread_element") {
        return None;
    }
    Some(values)
}

fn argument<'tree>(call: Syntax<'tree>, slot: &Slot) -> Option<Syntax<'tree>> {
    let Slot::Argument { index } = slot else {
        return None;
    };
    argument_values(call)?.get(*index).copied()
}

fn projection_argument<'tree>(
    call: Syntax<'tree>,
    slot: &Slot,
    values: &std::cell::OnceCell<Option<Vec<Syntax<'tree>>>>,
) -> Option<Syntax<'tree>> {
    let Slot::Argument { index } = slot else {
        return None;
    };
    values
        .get_or_init(|| argument_values(call))
        .as_ref()?
        .get(*index)
        .copied()
}

fn literal_key(node: Option<Syntax<'_>>, source: &str) -> StaticString {
    let Some(node) = node.filter(|n| n.kind() == "string") else {
        return StaticString::Unknown;
    };
    let value = text(node, source);
    let Some(value) = value.get(1..value.len().saturating_sub(1)) else {
        return StaticString::Unknown;
    };
    if value.is_empty()
        || value.len() > 256
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        StaticString::Unknown
    } else {
        StaticString::Literal {
            value: value.to_owned(),
        }
    }
}

fn direct_member<'tree>(object: Syntax<'tree>, source: &str, name: &str) -> Option<Syntax<'tree>> {
    supported_object(object)?;
    let mut found = None;
    for member in object.named_children(&mut object.walk()) {
        let key = member
            .child_by_field_name("key")
            .or_else(|| member.child_by_field_name("name"));
        if key.is_some_and(|key| text(key, source) == name) {
            if found.is_some() {
                return None;
            }
            found = Some(member);
        }
    }
    found
}

fn factory_members<'tree>(
    call: Syntax<'tree>,
    source: &str,
    index: usize,
    state: &str,
    actions: &str,
    arguments: &std::cell::OnceCell<Option<Vec<Syntax<'tree>>>>,
) -> Option<(
    String,
    Vec<crate::core::semantic::ObjectMember>,
    Vec<crate::core::semantic::ObjectMember>,
)> {
    let declaration = call.parent().filter(|n| {
        n.kind() == "variable_declarator" && n.child_by_field_name("value") == Some(call)
    })?;
    let binding = declaration
        .child_by_field_name("name")
        .filter(|n| n.kind() == "identifier")?;
    let options = projection_argument(call, &Slot::Argument { index }, arguments)
        .filter(|n| n.kind() == "object")?;
    let state = direct_member(options, source, state)?;
    let function = if state.kind() == "method_definition" {
        state
    } else {
        state.child_by_field_name("value")?
    };
    if !matches!(
        function.kind(),
        "arrow_function" | "function_expression" | "method_definition"
    ) {
        return None;
    }
    let mut returned = value_flow::final_return(function)?;
    if returned.kind() == "parenthesized_expression" {
        returned = returned.named_child(0)?;
    }
    let returned = returned_object(returned)?;
    let actions = direct_member(options, source, actions)?
        .child_by_field_name("value")
        .filter(|n| n.kind() == "object")?;
    supported_object(actions)?;
    if actions.named_children(&mut actions.walk()).any(|n| {
        n.kind() != "method_definition"
            || text(n, source).trim_start().starts_with("get ")
            || text(n, source).trim_start().starts_with("set ")
    }) {
        return None;
    }
    let ValueExpr::Object { members, .. } = value_flow::object_value(returned, source, None) else {
        return None;
    };
    let ValueExpr::Object {
        members: methods, ..
    } = value_flow::object_value(actions, source, None)
    else {
        return None;
    };
    Some((text(binding, source).to_owned(), members, methods))
}

/// Recognizes only the configured literal owner; API authority is proven after scope summaries.
pub(super) fn configured_projection_object(
    object: Syntax<'_>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
    enabled: bool,
) -> bool {
    if !enabled || object.kind() != "object" {
        return false;
    }
    let Some(arguments) = object.parent().filter(|n| n.kind() == "arguments") else {
        return false;
    };
    let Some(call) = arguments.parent().filter(|n| n.kind() == "call_expression") else {
        return false;
    };
    let Some(callee) = call
        .child_by_field_name("function")
        .and_then(|n| static_path(n, source))
    else {
        return false;
    };
    rules.iter().any(|rule| {
        manifests::semantic::projection_object_matches(
            rule,
            |api| api_shape(api, &callee),
            |slot| argument(call, slot) == Some(object),
            |_, _, _| false,
        )
    }) && supported_object(object).is_some()
}

pub(super) fn projected_members_object(
    object: Syntax<'_>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
    enabled: bool,
) -> bool {
    if !enabled || object.kind() != "object" {
        return false;
    }
    let mut parent = object.parent();
    // The admitted shapes have only an options pair, callable/return wrappers,
    // and the direct argument list between a member object and its factory.
    for _ in 0..8 {
        let Some(node) = parent else { return false };
        if node.kind() == "arguments" {
            let Some(call) = node.parent().filter(|n| n.kind() == "call_expression") else {
                return false;
            };
            let Some(callee) = call
                .child_by_field_name("function")
                .and_then(|n| static_path(n, source))
            else {
                return false;
            };
            return rules.iter().any(|rule| {
                manifests::semantic::projection_object_matches(
                    rule,
                    |api| api_shape(api, &callee),
                    |_| false,
                    |index, state_member, actions_member| {
                        let Some(options) = argument(call, &Slot::Argument { index }) else {
                            return false;
                        };
                        if object.parent().is_some_and(|pair| {
                            pair.kind() == "pair"
                                && pair.parent() == Some(options)
                                && pair
                                    .child_by_field_name("key")
                                    .is_some_and(|key| text(key, source) == actions_member)
                        }) {
                            return true;
                        }
                        let Some(state) = direct_member(options, source, state_member) else {
                            return false;
                        };
                        let function = if state.kind() == "method_definition" {
                            Some(state)
                        } else {
                            state.child_by_field_name("value")
                        };
                        let mut ancestor = object.parent();
                        for _ in 0..4 {
                            if ancestor == function {
                                return returned_object(object).is_some();
                            }
                            ancestor = ancestor.and_then(|n| n.parent());
                        }
                        false
                    },
                )
            });
        }
        if matches!(
            node.kind(),
            "program" | "class_declaration" | "variable_declarator"
        ) {
            return false;
        }
        parent = node.parent();
    }
    false
}

fn api_shape(api: &Api, callee: &str) -> bool {
    match api {
        Api::Imported { .. } => !callee.is_empty() && callee.split('.').count() <= 2,
        Api::Global { name } => callee == name,
        Api::GlobalMember { member, .. } => callee
            .rsplit_once('.')
            .is_some_and(|(_, name)| name == member),
        Api::ConstructedMember { .. } => false,
    }
}

pub(super) fn extract_projections(ctx: &SyntaxContext<'_, '_>, rules: &LanguageContext) {
    let kind = ctx.node.kind();
    let path = std::cell::OnceCell::new();
    let arguments = std::cell::OnceCell::new();
    let outermost_member = std::cell::OnceCell::new();
    let static_callee = || {
        path.get_or_init(|| {
            let expression = if kind == "call_expression" {
                ctx.node.child_by_field_name("function")
            } else {
                Some(ctx.node)
            };
            expression.and_then(|node| static_path(node, ctx.source))
        })
        .as_deref()
    };
    let setup = if kind == "call_expression"
        && SETUP_SUMMARIES.with(|summaries| !summaries.borrow().is_empty())
    {
        ctx.node
            .child_by_field_name("arguments")
            .map(|arguments| {
                arguments
                    .named_children(&mut arguments.walk())
                    .filter_map(|node| {
                        SETUP_SUMMARIES.with(|summaries| {
                            summaries
                                .borrow_mut()
                                .remove(&node.id())
                                .map(|members| (node.id(), members))
                        })
                    })
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    // Callback summaries belong to the call lifecycle, even when no rule publishes.
    if !rules.has_projection_rules_for_kind(kind) {
        return;
    }
    let position = SourcePosition {
        line: ctx.line(),
        column: ctx.node.start_position().column,
    };
    let Some(callee) = static_callee() else {
        return;
    };
    for (index, rule) in manifests::semantic::indexed_rules(
        &rules.syntax_rules,
        rules.projection_rule_candidates(kind, callee),
    ) {
        let Some(payload) = manifests::semantic::prepare_syntax_rule(
            rule,
            kind == "call_expression",
            callee,
            ctx.offset,
            api_shape,
            || {
                *outermost_member.get_or_init(|| {
                    kind != "member_expression"
                        || !ctx.node.parent().is_some_and(|parent| {
                            (parent.kind() == "member_expression"
                                && parent.child_by_field_name("object") == Some(ctx.node))
                                || (parent.kind() == "call_expression"
                                    && parent.child_by_field_name("function") == Some(ctx.node))
                        })
                })
            },
            |index, state, actions| {
                factory_members(ctx.node, ctx.source, index, state, actions, &arguments)
            },
            |index| {
                let callback = projection_argument(ctx.node, &Slot::Argument { index }, &arguments);
                let binding = ctx
                    .node
                    .parent()
                    .filter(|node| {
                        node.kind() == "variable_declarator"
                            && node.child_by_field_name("value") == Some(ctx.node)
                    })
                    .and_then(|node| node.child_by_field_name("name"))
                    .filter(|node| node.kind() == "identifier");
                callback
                    .and_then(|callback| setup.get(&callback.id()))
                    .zip(binding)
                    .map(|(members, binding)| {
                        (text(binding, ctx.source).to_owned(), members.clone())
                    })
            },
            |slot| {
                projection_argument(ctx.node, slot, &arguments).map(|node| {
                    (
                        SourcePosition {
                            line: node.start_position().row + ctx.offset + 1,
                            column: node.start_position().column,
                        },
                        static_path(node, ctx.source),
                    )
                })
            },
            |slot| {
                Some(literal_key(
                    projection_argument(ctx.node, slot, &arguments),
                    ctx.source,
                ))
            },
        ) else {
            continue;
        };
        PROJECTIONS.with(|pending| {
            pending.borrow_mut().push(ProjectionCandidate {
                owner: ctx.owner.to_owned(),
                rule: index,
                position,
                callee: callee.to_owned(),
                static_call: kind == "call_expression",
                payload,
            })
        });
    }
}

pub(super) fn finalize_projections(
    facts: &mut Facts,
    flows: Option<&mut FlowStore>,
    file: &FileContext,
) {
    SETUP_SUMMARIES.with(|summaries| summaries.borrow_mut().clear());
    let pending = PROJECTIONS.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    let file_import_mask = crate::engine::linker::interner::file_import_mask(facts);
    let mut fallback = FlowStore::default();
    if flows.is_none() {
        for node in &mut facts.nodes {
            fallback.ingest(node);
        }
    }
    let store = flows.as_deref().unwrap_or(&fallback);
    let mut output = HashMap::<String, ValueFlowFacts>::new();
    for candidate in pending {
        let Some(owner) = facts.nodes.iter().find(|node| node.id == candidate.owner) else {
            continue;
        };
        if matches!(
            store.state(owner),
            Some(crate::core::typed_facts::FlowState::Invalid(_))
        ) {
            continue;
        }
        let Some(rule) = file.rules.syntax_rules.get(candidate.rule) else {
            continue;
        };
        if let ProjectionPayload::Factory { binding, .. } = &candidate.payload {
            // Reject invalidated initializers before proving the API's lexical origin.
            // Both checks borrow the same completed owner flow.
            let Some((declaration, initializer)) = store
                .get(owner)
                .and_then(|flow| unique_initializer(flow, binding))
            else {
                continue;
            };
            if declaration.position >= candidate.position
                || initializer.position <= candidate.position
                || !matches!(&initializer.value, ValueExpr::Call { callee, .. } if callee == &candidate.callee)
            {
                continue;
            }
        }
        let api = match manifests::semantic::syntax_rule_api(rule) {
            Some(Ok(api)) => Some(api),
            Some(Err(root)) => {
                if !unshadowed(&lexical_scopes(owner, facts), root, facts) {
                    continue;
                }
                if candidate.static_call {
                    for reference in &mut facts.references {
                        if reference.kind == "calls"
                            && reference.source == candidate.owner
                            && reference.line == candidate.position.line
                            && reference.column == candidate.position.column
                            && reference.expression == candidate.callee
                        {
                            reference.dynamic = false;
                        }
                    }
                }
                None
            }
            None => continue,
        };
        if api.is_some_and(|api| {
            !api_proven(
                api,
                &candidate.callee,
                owner,
                candidate.position,
                facts,
                store,
                file_import_mask,
            )
        }) {
            continue;
        }
        let flow = output.entry(candidate.owner).or_default();
        manifests::semantic::bind_receiver_type(
            rule,
            candidate.position,
            candidate.payload,
            facts,
            flow,
        );
    }
    let typed = flows.is_some();
    let store = flows.unwrap_or(&mut fallback);
    for (owner, extra) in output {
        let mut flow = store.take(&owner).unwrap_or_default();
        if let Some(extra) = extra.framework {
            let mut extra = *extra;
            extra.facts.retain(|fact| {
                matches!(
                    fact,
                    crate::core::semantic::FrameworkFact::FactoryResult(_)
                        | crate::core::semantic::FrameworkFact::NamedBinding(_)
                )
            });
            if !extra.facts.is_empty() {
                flow.framework
                    .get_or_insert_with(Default::default)
                    .facts
                    .append(&mut extra.facts);
            }
        }
        store.insert(owner, flow);
    }
    if !typed {
        for node in &mut facts.nodes {
            if let Some(state) = store.state(node) {
                node.details["value_flow"] = serde_json::json!(state.view());
            }
        }
    }
}

pub(super) fn static_member_callee(function: Syntax<'_>, source: &str) -> Option<String> {
    if function.kind() != "subscript_expression" {
        let mut node = function;
        let mut segments = Vec::new();
        for _ in 0..64 {
            match node.kind() {
                "identifier" | "this" | "super" => {
                    segments.push(text(node, source));
                    segments.reverse();
                    return Some(segments.join("."));
                }
                "member_expression" => {
                    let property = node
                        .child_by_field_name("property")
                        .filter(|property| property.kind() == "property_identifier")?;
                    segments.push(text(property, source));
                    node = node.child_by_field_name("object")?;
                }
                _ => return None,
            }
        }
        return None;
    }
    let receiver = function
        .child_by_field_name("object")
        .filter(|node| node.kind() == "identifier")?;
    let member = match function.kind() {
        "subscript_expression" => {
            let index = function
                .child_by_field_name("index")
                .filter(|node| node.kind() == "string")?;
            let literal = text(index, source);
            let quoted = literal.as_bytes();
            if quoted.len() < 3
                || !matches!(quoted[0], b'\'' | b'"')
                || quoted[0] != quoted[quoted.len() - 1]
            {
                return None;
            }
            &literal[1..literal.len() - 1]
        }
        _ => return None,
    };
    let mut bytes = member.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
    {
        return None;
    }
    Some(format!("{}.{}", text(receiver, source), member))
}

pub(super) fn supported_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    // The helper is called for each direct member. Keep the object-wide check
    // bounded, and decline unusually large objects rather than repeatedly
    // scanning an unbounded member list on the extraction hot path.
    let mut cursor = object.walk();
    let mut count = 0;
    for member in object.named_children(&mut cursor) {
        count += 1;
        if count > 128
            || member.kind() == "spread_element"
            || (member.kind() == "pair"
                && !member
                    .child_by_field_name("key")
                    .is_some_and(|key| matches!(key.kind(), "property_identifier" | "identifier")))
        {
            return None;
        }
    }
    Some(object)
}

fn unparenthesized_object(object: Syntax<'_>) -> Syntax<'_> {
    object
        .parent()
        .filter(|parent| {
            parent.kind() == "parenthesized_expression"
                && parent.named_child_count() == 1
                && parent.named_child(0) == Some(object)
        })
        .unwrap_or(object)
}

pub(super) fn returned_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    if object.kind() != "object" {
        return None;
    }
    let expression = unparenthesized_object(object);
    let enclosing = expression.parent()?;
    let function = if enclosing.kind() == "return_statement" {
        let body = enclosing.parent()?;
        if body.kind() != "statement_block" {
            return None;
        }
        body.parent()?
    } else if enclosing.kind() == "arrow_function" {
        enclosing
    } else {
        return None;
    };
    if value_flow::final_return(function) != Some(expression) {
        return None;
    }
    supported_object(object)
}

fn factory_call_callee<'a>(object: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    if object.kind() != "object" {
        return None;
    }
    let enclosing = unparenthesized_object(object).parent()?;
    if enclosing.kind() != "arguments" {
        return None;
    }
    let call = enclosing.parent().filter(|n| n.kind() == "call_expression")?;
    call.child_by_field_name("function")
        .map(|fun| text(fun, source))
}

pub(super) fn configured_factory_object<'tree>(
    object: Syntax<'tree>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
) -> Option<Syntax<'tree>> {
    let callee = factory_call_callee(object, source)?;
    rules
        .iter()
        .any(|rule| manifests::semantic::object_factory_matches(rule, callee))
        .then(|| supported_object(object))
        .flatten()
}

pub(super) fn same_object_prepared<'tree>(
    object: Syntax<'tree>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
    factory_members: bool,
    object_definitions: bool,
) -> Option<Syntax<'tree>> {
    returned_object(object)
        .or_else(|| configured_factory_object(object, source, rules))
        .or_else(|| {
            if configured_projection_object(object, source, rules, object_definitions) {
                Some(object)
            } else if projected_members_object(object, source, rules, factory_members) {
                supported_object(object)
            } else {
                None
            }
        })
}

pub(super) fn projected_object<'tree>(
    object: Syntax<'tree>,
    source: &str,
    context: &LanguageContext,
) -> Option<Syntax<'tree>> {
    if projected_members_object(
        object,
        source,
        &context.syntax_rules,
        context.has_factory_members,
    ) {
        supported_object(object)
    } else {
        configured_projection_object(
            object,
            source,
            &context.syntax_rules,
            context.has_object_call_definitions,
        )
        .then_some(object)
    }
}

pub(super) fn same_object_receiver<'tree>(
    node: Syntax<'tree>,
    source: &str,
    context: &LanguageContext,
) -> Option<Syntax<'tree>> {
    if node.kind() == "function_expression" {
        let pair = node.parent()?;
        if pair.kind() == "pair" && pair.child_by_field_name("value") == Some(node) {
            return same_object_prepared(
                pair.parent()?,
                source,
                &context.syntax_rules,
                context.has_factory_members,
                context.has_object_call_definitions,
            );
        }
        return None;
    }
    let member = if node.kind() == "method_definition" {
        node
    } else if node.kind() == "arrow_function" {
        let mut current = node.parent();
        loop {
            let parent = current?;
            match parent.kind() {
                "method_definition" => break parent,
                "function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_declaration"
                | "class_declaration"
                | "class" => return None,
                _ => current = parent.parent(),
            }
        }
    } else {
        return None;
    };
    same_object_prepared(
        member.parent()?,
        source,
        &context.syntax_rules,
        context.has_factory_members,
        context.has_object_call_definitions,
    )
}

pub(super) fn factory_ownership(
    object: Syntax<'_>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
) -> Option<crate::core::semantic::ObjectOwnershipFact> {
    configured_factory_object(object, source, rules)?;
    let callee = factory_call_callee(object, source)?;
    rules
        .iter()
        .find_map(|rule| manifests::semantic::execute_object_factory_rule(rule, callee))
}

pub(super) fn object_marker(
    object: Syntax<'_>,
    source: &str,
    rules: &[manifests::syntax::SyntaxRule],
) -> String {
    let start = object.start_position();
    if configured_factory_object(object, source, rules).is_none() {
        return format!("object_return:{}:{}", start.row + 1, start.column);
    }
    let binding = factory_call_callee(object, source)
        .and_then(|callee| callee.split('.').next())
        .unwrap_or("");
    let guard = rules
        .iter()
        .any(|rule| manifests::semantic::object_factory_guard(rule, binding));
    format!(
        "factory_object:{}:{}:{}",
        start.row + 1,
        start.column,
        if guard { binding } else { "" }
    )
}

fn is_export_default_config_call(node: Syntax<'_>, source: &str, config_callee: &str) -> bool {
    node.kind() == "call_expression"
        && node.parent().is_some_and(|parent| {
            parent.kind() == "export_statement"
                && text(parent, source)
                    .trim_start()
                    .starts_with("export default")
        })
        && node
            .child_by_field_name("function")
            .is_some_and(|callee| text(callee, source) == config_callee)
}

pub(super) fn configuration_module_declared(
    node: Syntax<'_>,
    source: &str,
    config_callee: &str,
    modules_key: &str,
    module_name: &str,
) -> bool {
    if !is_export_default_config_call(node, source, config_callee) {
        return false;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    let mut values = args.named_children(&mut cursor);
    let Some(config) = values.next().filter(|value| value.kind() == "object") else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let mut modules = None;
    for pair in config.named_children(&mut config.walk()) {
        if pair.kind() == "comment" {
            continue;
        }
        if pair.kind() != "pair" {
            return false;
        }
        if pair
            .child_by_field_name("key")
            .is_some_and(|key| text(key, source) == modules_key)
        {
            if modules.is_some() {
                return false;
            }
            modules = pair
                .child_by_field_name("value")
                .filter(|value| value.kind() == "array");
            if modules.is_none() {
                return false;
            }
        }
    }
    let Some(modules) = modules else {
        return false;
    };
    let mut found = false;
    for item in modules.named_children(&mut modules.walk()) {
        if item.kind() != "string" || text(item, source).contains('\\') {
            return false;
        }
        found |= text(item, source).trim_matches(['\'', '"']) == module_name;
    }
    found
}

pub(super) fn configuration_components_enabled(
    node: Syntax<'_>,
    source: &str,
    config_callee: &str,
    enabled_key: &str,
) -> Option<bool> {
    if !is_export_default_config_call(node, source, config_callee) {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    if arguments.named_child_count() != 1 {
        return Some(false);
    }
    let config = arguments.named_child(0)?;
    if config.kind() != "object" {
        return Some(false);
    }
    let mut enabled = true;
    let mut seen = false;
    for property in config.named_children(&mut config.walk()) {
        if property.kind() == "comment" {
            continue;
        }
        if property.kind() != "pair" {
            return Some(false);
        }
        let Some(key) = property.child_by_field_name("key") else {
            return Some(false);
        };
        let name = match key.kind() {
            "property_identifier" => text(key, source),
            "string" => text(key, source).trim_matches(['\'', '"']),
            _ => return Some(false),
        };
        if name != enabled_key {
            continue;
        }
        if seen {
            return Some(false);
        }
        seen = true;
        enabled = property
            .child_by_field_name("value")
            .is_some_and(|value| text(value, source) == "true");
    }
    Some(enabled)
}

fn configuration_callable_imports(
    node: Syntax<'_>,
    source: &str,
    config_callee: &str,
    descriptor: &manifests::syntax::CallableImports,
) -> Option<crate::core::semantic::CallableImportConfiguration> {
    fn property<'tree>(
        object: Syntax<'tree>,
        source: &str,
        wanted: &str,
    ) -> Option<Option<Syntax<'tree>>> {
        if object.kind() != "object" || object.has_error() {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let mut found = None;
        for pair in object.named_children(&mut object.walk()) {
            if pair.kind() == "comment" {
                continue;
            }
            if pair.kind() != "pair" {
                return None;
            }
            let key = pair.child_by_field_name("key")?;
            let raw = text(key, source);
            let name = match key.kind() {
                "property_identifier" => raw,
                "string" if !raw.contains('\\') => {
                    let quote = raw.as_bytes().first().copied()?;
                    if !matches!(quote, b'\'' | b'"')
                        || raw.as_bytes().last().copied() != Some(quote)
                    {
                        return None;
                    }
                    raw.get(1..raw.len().checked_sub(1)?)?
                }
                _ => return None,
            };
            if !seen.insert(name) {
                return None;
            }
            let value = pair.child_by_field_name("value")?;
            if name == wanted {
                found = Some(value);
            }
        }
        Some(found)
    }

    fn path<'tree>(
        object: Syntax<'tree>,
        source: &str,
        key: &str,
    ) -> Option<Option<Syntax<'tree>>> {
        let mut current = Some(object);
        for segment in key.split('.') {
            current = match current {
                Some(object) => property(object, source, segment)?,
                None => return Some(None),
            };
        }
        Some(current)
    }

    fn flag(object: Syntax<'_>, source: &str, key: &str) -> Option<bool> {
        match property(object, source, key)? {
            None => Some(true),
            Some(value) => match value.kind() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
        }
    }

    if !is_export_default_config_call(node, source, config_callee) || node.has_error() {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let mut values = arguments
        .named_children(&mut cursor)
        .filter(|value| value.kind() != "comment");
    let config = values.next()?;
    if values.next().is_some() {
        return None;
    }
    // Even absent options require a fully static direct property boundary.
    property(config, source, &descriptor.options_key)?;
    for unsupported in &descriptor.unsupported_keys {
        if path(config, source, unsupported)?.is_some() {
            return None;
        }
    }
    let (enabled, scan) = match property(config, source, &descriptor.options_key)? {
        None => (true, true),
        Some(options) => (
            flag(options, source, &descriptor.enabled_key)?,
            flag(options, source, &descriptor.scan_key)?,
        ),
    };
    Some(crate::core::semantic::CallableImportConfiguration { enabled, scan })
}

pub(super) fn extract_configuration(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    rules: &LanguageContext,
    projections: &mut HashMap<String, ValueFlowFacts>,
) {
    let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") else {
        return;
    };
    for rule in &rules.syntax_rules {
        let Some(fact) = manifests::semantic::execute_configuration_rule(
            rule,
            &module.path,
            |callee, key| configuration_components_enabled(ctx.node, ctx.source, callee, key),
            |callee, key, name| {
                configuration_module_declared(ctx.node, ctx.source, callee, key, name)
            },
            |callee, descriptor| {
                configuration_callable_imports(ctx.node, ctx.source, callee, descriptor)
            },
        ) else {
            continue;
        };
        projections
            .entry(module.id.clone())
            .or_default()
            .push_component_configuration(fact);
    }
}
