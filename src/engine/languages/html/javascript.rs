use super::*;
use crate::core::models::{Diagnostic, Edge};
use crate::engine::languages::template::TemplateMasker;

pub(super) struct ScriptExtraction<'a> {
    pub(super) profile: &'a dyn LanguageProfile,
    pub(super) parser: &'a mut tree_sitter::Parser,
    pub(super) path: &'a str,
    pub(super) source: &'a str,
    pub(super) module: &'a str,
    pub(super) global_scope: &'a mut Option<usize>,
    pub(super) facts: &'a mut Facts,
    pub(super) flows: Option<&'a mut crate::core::typed_facts::FlowStore>,
    pub(super) rules: &'a Arc<LanguageContext>,
}

pub(super) struct Island<'a> {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) position: tree_sitter::Point,
    pub(super) kind: &'a str,
}

pub(super) fn extract_island(
    context: &mut ScriptExtraction<'_>,
    island: Island<'_>,
) -> Result<Option<usize>> {
    let Island {
        start,
        end,
        position,
        kind,
    } = island;
    let original = &context.source[start..end];
    let script = TemplateMasker::mask(original, |_| {}, None);
    if script.trim().is_empty() {
        return Ok(None);
    }
    let tree = ast::parse_island(context.parser, script.as_ref(), start, position)
        .context("embedded JavaScript parse cancelled")?;
    let row = position.row;
    let column = position.column;
    let root = tree.root_node();
    let line = row + 1;
    let global_owner = format!("html-js:{}:classic", context.path);
    let global_qualname = format!("{}.classic", context.module);
    let global_index = ensure_global_scope(
        context,
        &global_owner,
        &global_qualname,
        position,
        root.end_position().row + 1,
    );
    let (owner, scope) = if kind == "classic" {
        (global_owner.clone(), global_qualname.clone())
    } else {
        (
            format!("html-js:{}:{start}:{kind}", context.path),
            format!("{global_qualname}.{kind}@{start}"),
        )
    };
    let destination = &mut *context.facts;
    let mut island = Facts::default();
    let mut island_flows = crate::core::typed_facts::FlowStore::default();
    let facts = &mut island;
    facts.nodes.push(super::source_node(
        context.path,
        "javascript",
        super::NodeFields {
            id: owner.clone(),
            kind: "template_scope".into(),
            name: format!("{kind}@{start}"),
            qualname: scope.clone(),
            line,
            end_line: root.end_position().row + 1,
            details: json!({"column":column,"embedded_language":"javascript","lexical_boundary":true,"html_import_owner":format!("module:{}", context.path)}),
        },
    ));
    if kind == "handler" {
        facts.nodes[0].details["html_handler"] = json!(true);
    }
    if kind != "classic" {
        facts.edges.push(Edge {
            src: global_owner,
            dst: owner.clone(),
            kind: "contains".into(),
            path: context.path.into(),
            line,
            evidence: format!("embedded JavaScript {kind}"),
            confidence: "exact".into(),
        });
    }
    let errors = facts.errors.len();
    ast::extract_tree_with_language_context(
        context.profile,
        root,
        context.path,
        context.source,
        &scope,
        &owner,
        facts,
        Some(&mut island_flows),
        context.rules,
    );
    if matches!(script, std::borrow::Cow::Owned(_)) {
        facts.errors.truncate(errors);
    }
    for diagnostic in &mut facts.errors[errors..] {
        diagnostic.message = format!("embedded JavaScript: {}", diagnostic.message);
    }
    let require_imports = require_import_positions(root, context.source, start, end);
    if !require_imports.is_empty() {
        facts.nodes[0].details["require_imports"] = Value::Array(require_imports);
    }
    if kind == "handler" {
        if let Some(scope) = facts.nodes.iter_mut().find(|node| node.id == owner) {
            let mut bindings: std::collections::BTreeSet<String> =
                scope.binding_names().map(str::to_owned).collect();
            bindings.insert("event".into());
            let rebindings = scope.rebinding_names().map(str::to_owned).collect();
            crate::core::typed_facts::NodeSemanticEntry::set_bindings(
                scope,
                bindings.into_iter().collect(),
                rebindings,
            );
        }
    }
    let mut ids = std::collections::HashMap::new();
    for node in &mut facts.nodes {
        if node.id != owner {
            let previous = std::mem::take(&mut node.id);
            node.id = format!("{previous}:html:{start}:{kind}");
            island_flows.rename(&previous, node.id.clone());
            ids.insert(previous, node.id.clone());
        }
    }
    island_flows.remap_nodes(&ids);
    for reference in &mut facts.references {
        if let Some(id) = ids.get(&reference.source) {
            reference.source.clone_from(id);
        }
    }
    for edge in &mut facts.edges {
        if let Some(id) = ids.get(&edge.src) {
            edge.src.clone_from(id);
        }
        if let Some(id) = ids.get(&edge.dst) {
            edge.dst.clone_from(id);
        }
    }
    let node_count = facts.nodes.len();
    let offset = destination.nodes.len();
    let scope_node_index = if kind == "classic" {
        global_index
    } else {
        offset
    };
    let node_mapping: std::collections::HashMap<usize, usize> = facts
        .pending_flow_nodes
        .iter()
        .copied()
        .chain(facts.semantic.entries().map(|(index, _)| *index))
        .map(|index| {
            let target = if kind == "classic" {
                if index == 0 {
                    global_index
                } else if index == node_count - 1 {
                    offset
                } else {
                    offset + index
                }
            } else {
                offset + index
            };
            (index, target)
        })
        .collect();
    if kind == "classic" {
        let mut fragment = facts.nodes.swap_remove(0);
        let target = &mut destination.nodes[global_index];
        if let Some(incoming) = fragment.native_semantic.take() {
            target
                .native_semantic
                .get_or_insert_with(Default::default)
                .merge_from(*incoming);
        }
        merge_global_details(&mut target.details, fragment.details);
        crate::core::typed_facts::NodeSemanticEntry::ingest(target);
        if let Some(flows) = context.flows.as_deref_mut() {
            flows.merge_classic(&target.id, &mut island_flows);
        } else {
            let mut public_flows = crate::core::typed_facts::FlowStore::default();
            public_flows.ingest(target);
            public_flows.merge_classic(&target.id, &mut island_flows);
            if let Some(state) = public_flows.state(target) {
                target.details["value_flow"] =
                    serde_json::to_value(state.view()).expect("parser-owned classic flow");
            }
        }
    }
    if context.flows.is_none() {
        for node in &mut facts.nodes {
            if let Some(state) = island_flows.state(node) {
                node.details["value_flow"] =
                    serde_json::to_value(state.view()).expect("parser-owned embedded flow");
            }
        }
    }
    for index in std::mem::take(&mut facts.pending_flow_nodes) {
        destination.pending_flow_nodes.push(node_mapping[&index]);
    }
    destination
        .semantic
        .append_remapped(&mut facts.semantic, &node_mapping, &ids);
    destination.nodes.append(&mut facts.nodes);
    destination.edges.append(&mut facts.edges);
    destination.references.append(&mut facts.references);
    destination.docs.append(&mut facts.docs);
    destination.errors.append(&mut facts.errors);
    if let Some(flows) = context.flows.as_deref_mut() {
        flows.append(&mut island_flows);
    }
    Ok(Some(scope_node_index))
}

fn require_import_positions(
    root: Syntax<'_>,
    source: &str,
    start: usize,
    end: usize,
) -> Vec<Value> {
    let mut positions = Vec::new();
    for (offset, _) in source[start..end].match_indices("require") {
        let function_start = start + offset;
        let function_end = function_start + "require".len();
        let Some(function) = root.descendant_for_byte_range(function_start, function_end) else {
            continue;
        };
        if function.kind() != "identifier"
            || function.start_byte() != function_start
            || function.end_byte() != function_end
        {
            continue;
        }
        let Some(call) = function.parent().filter(|parent| {
            parent.kind() == "call_expression"
                && parent.child_by_field_name("function") == Some(function)
        }) else {
            continue;
        };

        let call_position = call.start_position();
        if let Some(parent) = call.parent().filter(|parent| match parent.kind() {
            "variable_declarator" => parent.child_by_field_name("value") == Some(call),
            "assignment_expression" => parent.child_by_field_name("right") == Some(call),
            _ => false,
        }) {
            let position = parent.start_position();
            positions.push(json!([
                position.row + 1,
                position.column,
                call_position.row + 1,
                call_position.column
            ]));
        }
        positions.push(json!([
            call_position.row + 1,
            call_position.column,
            call_position.row + 1,
            call_position.column
        ]));
    }
    positions
}

fn named_attribute_bindings<'a>(
    rules: &'a LanguageContext,
    name: &'a str,
) -> impl Iterator<
    Item = (
        usize,
        &'a manifests::syntax::SyntaxRule,
        &'a String,
        &'a String,
        &'a str,
    ),
> + 'a {
    rules
        .syntax_rules
        .iter()
        .enumerate()
        .filter_map(move |(index, rule)| {
            let manifests::syntax::SyntaxOperation::NamedBinding {
                domain,
                binding:
                    manifests::projections::NamedBindingForm::AttributeDefinition { prefix, scope },
            } = &rule.operation
            else {
                return None;
            };
            name.get(..prefix.len())
                .filter(|value| value.eq_ignore_ascii_case(prefix))
                .map(|_| (index, rule, domain, scope, &name[prefix.len()..]))
        })
}

pub(super) fn extract_handlers(context: &mut ScriptExtraction<'_>, tag: Syntax<'_>) -> Result<()> {
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
    {
        let Some(name) = attribute
            .named_child(0)
            .map(|node| text(node, context.source))
        else {
            continue;
        };
        let named_rule_index = named_attribute_bindings(context.rules, name)
            .next()
            .map(|(index, _, _, _, _)| index);
        let handler = if name.len() > 2
            && (name.starts_with("on")
                || name.starts_with("ON")
                || name.starts_with("On")
                || name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("on")))
        {
            Some(("handler".to_owned(), None))
        } else if let Some((kind, origin)) = context.rules.syntax_rules.iter().find_map(|rule| {
            let manifests::syntax::SyntaxOperation::HtmlScriptAttributes {
                event_prefixes,
                object_attribute,
                event_scope,
                object_scope,
            } = &rule.operation
            else {
                return None;
            };
            if event_prefixes.iter().any(|prefix| {
                name.get(..prefix.len())
                    .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
            }) {
                Some(event_scope.as_str())
            } else if name.eq_ignore_ascii_case(object_attribute) {
                let val = attribute
                    .named_child(1)
                    .map(|value| text(value, context.source).trim())
                    .unwrap_or("");
                (val.contains('{')
                    || val.contains("()")
                    || val.contains("=>")
                    || val.contains("function"))
                .then_some(object_scope.as_str())
            } else {
                None
            }
            .map(|kind| {
                (
                    kind,
                    crate::core::semantic::ExtractionRuleOrigin {
                        pack: rule.pack.clone(),
                        rule: rule.id.clone(),
                    },
                )
            })
        }) {
            Some((kind.to_owned(), Some(origin)))
        } else if let Some(index) = named_rule_index {
            context.rules.syntax_rules.get(index).and_then(|rule| {
                let manifests::syntax::SyntaxOperation::NamedBinding {
                    binding:
                        manifests::projections::NamedBindingForm::AttributeDefinition { scope, .. },
                    ..
                } = &rule.operation
                else {
                    return None;
                };
                Some((scope.clone(), None))
            })
        } else {
            None
        };
        let Some((kind, origin)) = handler else {
            continue;
        };
        let Some(value) = attribute.named_child(1) else {
            continue;
        };
        let raw = text(value, context.source);
        let quoted = raw.starts_with(['\'', '"']);
        let start = value.start_byte() + usize::from(quoted);
        let end = value.end_byte().saturating_sub(usize::from(quoted));
        if end < start {
            continue;
        }
        if context.source[start..end]
            .match_indices('&')
            .any(|(offset, _)| super::is_html_entity(&context.source[start + offset..end]))
        {
            context.facts.errors.push(Diagnostic {
                path: context.path.into(),
                line: value.start_position().row + 1,
                message: "HTML-encoded JavaScript handler requires decoding".into(),
            });
            continue;
        }
        let mut position = value.start_position();
        position.column += usize::from(quoted);
        let errors_before = context.facts.errors.len();
        let Some(node_index) = extract_island(
            context,
            Island {
                start,
                end,
                position,
                kind: &kind,
            },
        )?
        else {
            continue;
        };
        let named_static = context.facts.errors.len() == errors_before
            && !context.source[start..end].trim().is_empty()
            && matches!(
                TemplateMasker::mask(&context.source[start..end], |_| {}, None),
                std::borrow::Cow::Borrowed(_)
            );
        if origin.is_some() || named_rule_index.is_some() {
            let owner = format!("html-js:{}:{start}:{kind}", context.path);
            if let Some(node) = context.facts.nodes.get_mut(node_index) {
                let enrich = |flow: &mut crate::core::semantic::ValueFlowFacts| {
                    if let Some(origin) = &origin {
                        flow.push_rule_origin(origin.clone());
                    }
                    for (_, rule, domain, _, key) in named_attribute_bindings(context.rules, name) {
                        use crate::core::semantic::{
                            ExtractionRuleOrigin, NamedBindingFact, NamedBindingRole,
                            NamedTargetKind, SourcePosition, StaticString, SymbolOperand,
                        };
                        if !named_static
                            || key.is_empty()
                            || key.len() > 256
                            || !key
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':'))
                        {
                            continue;
                        }
                        if flow.named_bindings().count() == 128 {
                            flow.fact_limit_exceeded = true;
                            break;
                        }
                        flow.push_named_binding(NamedBindingFact {
                            origin: ExtractionRuleOrigin {
                                pack: rule.pack.clone(),
                                rule: rule.id.clone(),
                            },
                            position: SourcePosition {
                                line: attribute.start_position().row + 1,
                                column: attribute.start_position().column,
                            },
                            domain: (*domain).clone(),
                            key: StaticString::Literal { value: key.into() },
                            role: NamedBindingRole::Definition {
                                target: SymbolOperand::Node { id: owner.clone() },
                                target_kind: NamedTargetKind::ListenerScope,
                            },
                        });
                    }
                };
                if let Some(flows) = context.flows.as_deref_mut() {
                    if let Some(flow) = flows.get_mut(&owner) {
                        enrich(flow);
                    } else {
                        node.details["value_flow"] = Value::Null;
                        let mut flow = crate::core::semantic::ValueFlowFacts::default();
                        enrich(&mut flow);
                        flows.insert(owner.clone(), flow);
                    }
                } else {
                    let mut flow = node
                        .details
                        .get("value_flow")
                        .filter(|value| !value.is_null())
                        .map(|value| {
                            serde_json::from_value::<crate::core::semantic::ValueFlowFacts>(
                                value.clone(),
                            )
                            .expect("parser-owned script flow")
                        })
                        .unwrap_or_default();
                    enrich(&mut flow);
                    node.details["value_flow"] = json!(flow);
                }
            }
        }
    }
    Ok(())
}

fn ensure_global_scope(
    context: &mut ScriptExtraction<'_>,
    owner: &str,
    qualname: &str,
    position: tree_sitter::Point,
    end_line: usize,
) -> usize {
    if let Some(index) = *context.global_scope {
        let node = &mut context.facts.nodes[index];
        if (position.row + 1, position.column)
            < (
                node.line,
                node.details["column"].as_u64().unwrap_or(0) as usize,
            )
        {
            node.line = position.row + 1;
            node.details["column"] = json!(position.column);
        }
        node.end_line = node.end_line.max(end_line);
        return index;
    }
    let next = context.facts.nodes.len();
    context.facts.nodes.push(super::source_node(
        context.path,
        "javascript",
        super::NodeFields {
            id: owner.into(),
            kind: "template_scope".into(),
            name: "classic scripts".into(),
            qualname: qualname.into(),
            line: position.row + 1,
            end_line,
            details: json!({"column":position.column,"embedded_language":"javascript","classic_global":true,"lexical_boundary":true,"html_import_owner":format!("module:{}", context.path)}),
        },
    ));
    context.facts.edges.push(Edge {
        src: format!("module:{}", context.path),
        dst: owner.into(),
        kind: "contains".into(),
        path: context.path.into(),
        line: position.row + 1,
        evidence: "classic JavaScript environment".into(),
        confidence: "exact".into(),
    });
    *context.global_scope = Some(next);
    next
}

fn merge_global_details(target: &mut Value, mut fragment: Value) {
    for key in ["bindings", "rebindings", "require_imports"] {
        if let Value::Array(mut values) = fragment[key].take() {
            if values.is_empty() {
                continue;
            }
            if !target[key].is_array() {
                target[key] = json!([]);
            }
            target[key]
                .as_array_mut()
                .expect("array initialized")
                .append(&mut values);
        }
    }
}

pub(super) fn finish(facts: &mut Facts) {
    let globals: std::collections::HashSet<_> = facts
        .nodes
        .iter()
        .filter(|node| node.details["classic_global"] == true)
        .map(|node| node.id.as_str())
        .collect();
    let require_declarations: std::collections::HashSet<_> = facts
        .nodes
        .iter()
        .filter(|node| node.name == "require" && matches!(node.kind.as_str(), "function" | "class"))
        .map(|node| node.id.as_str())
        .collect();
    let shadowed_require = facts.nodes.iter().any(|node| {
        globals.contains(node.id.as_str())
            && node.binding_names().any(|candidate| candidate == "require")
    }) || facts.edges.iter().any(|edge| {
        edge.kind == "contains"
            && globals.contains(edge.src.as_str())
            && require_declarations.contains(edge.dst.as_str())
    });
    if shadowed_require {
        let positions: std::collections::HashMap<_, _> = facts
            .nodes
            .iter()
            .flat_map(|node| {
                node.details["require_imports"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(|position| {
                Some((
                    (
                        position[0].as_u64()? as usize,
                        position[1].as_u64()? as usize,
                    ),
                    (
                        position[2].as_u64()? as usize,
                        position[3].as_u64()? as usize,
                    ),
                ))
            })
            .collect();
        let mut calls = std::collections::HashSet::new();
        facts.references.retain_mut(|reference| {
            if reference.kind != "imports" {
                return true;
            }
            match positions.get(&(reference.line, reference.column)) {
                Some(&(line, column)) => {
                    reference.kind = "calls".into();
                    reference.expression = "require".into();
                    reference.line = line;
                    reference.column = column;
                    reference.alias = None;
                    reference.module = None;
                    calls.insert((reference.source.clone(), line, column))
                }
                None => true,
            }
        });
    }
    for node in &mut facts.nodes {
        if let Some(details) = node.details.as_object_mut() {
            details.remove("require_imports");
        }
    }
    for node in facts
        .nodes
        .iter_mut()
        .filter(|node| node.details["classic_global"] == true)
    {
        for key in ["bindings", "rebindings"] {
            if let Some(values) = node.details[key].as_array_mut() {
                values.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
                values.dedup();
            }
        }
        for key in ["bindings", "fields"] {
            if let Some(values) = node
                .details
                .get_mut("value_flow")
                .and_then(serde_json::Value::as_object_mut)
                .and_then(|flow| flow.get_mut(key))
                .and_then(serde_json::Value::as_array_mut)
            {
                values.sort_by(|left, right| {
                    (
                        left["position"]["line"].as_u64(),
                        left["position"]["column"].as_u64(),
                        left["name"].as_str(),
                    )
                        .cmp(&(
                            right["position"]["line"].as_u64(),
                            right["position"]["column"].as_u64(),
                            right["name"].as_str(),
                        ))
                });
            }
        }
        if let Some(values) = node
            .details
            .get_mut("value_flow")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|flow| flow.get_mut("immutable_initializers"))
            .and_then(serde_json::Value::as_array_mut)
        {
            values.sort_by(|left, right| {
                (left["line"].as_u64(), left["column"].as_u64())
                    .cmp(&(right["line"].as_u64(), right["column"].as_u64()))
            });
        }
    }
}

pub(super) fn finish_typed(facts: &mut Facts, flows: &mut crate::core::typed_facts::FlowStore) {
    finish(facts);
    for node in facts
        .nodes
        .iter()
        .filter(|node| node.details["classic_global"] == true)
    {
        if let Some(flow) = flows.get_mut(&node.id) {
            flow.bindings.sort_by(|left, right| {
                (left.position, &left.name).cmp(&(right.position, &right.name))
            });
            flow.immutable_initializers.sort_unstable();
            flow.fields.sort_by(|left, right| {
                (left.position, &left.name).cmp(&(right.position, &right.name))
            });
        }
    }
}

pub(super) fn script_kind(node: Syntax<'_>, source: &str) -> &'static str {
    let mut cursor = node.walk();
    let module = node
        .named_children(&mut cursor)
        .find(|node| node.kind() == "start_tag")
        .and_then(|tag| super::attribute_value(tag, source, "type"))
        .is_some_and(|value| value.eq_ignore_ascii_case("module"));
    if module {
        "module"
    } else {
        "classic"
    }
}
