//! Vue template scopes and JavaScript expression islands.
//!
//! HTML owns element boundaries; the JavaScript parser owns expression syntax.
//! Source offsets are retained rather than recovering identifiers from tokens.

use super::*;
use crate::core::models::{is_test, Diagnostic, Edge};
use crate::core::semantic::{BindingFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts};
use crate::engine::languages::template::TemplateMasker;

struct Document<'a> {
    path: &'a str,
    source: &'a str,
    module: &'a str,
    line_starts: Vec<usize>,
    scopes: std::collections::HashMap<String, String>,
    scope_bindings: std::collections::HashMap<String, (String, Vec<String>)>,
    components: std::collections::HashMap<String, String>,
    typed_iterables: &'a BTreeMap<String, TypeExpr>,
    flows: Option<&'a mut crate::core::typed_facts::FlowStore>,
}

pub(super) struct TemplateBindings<'a> {
    pub setup_positions: &'a [(Position, Position)],
    pub registered_components: &'a BTreeMap<String, String>,
    pub typed_iterables: &'a BTreeMap<String, TypeExpr>,
}

impl Document<'_> {
    fn point(&self, offset: usize) -> tree_sitter::Point {
        let row = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        tree_sitter::Point::new(row, offset - self.line_starts[row])
    }
}

pub(super) fn extract(
    path: &str,
    source: &str,
    module: &str,
    setup: TemplateBindings<'_>,
    facts: &mut Facts,
    flows: Option<&mut crate::core::typed_facts::FlowStore>,
) -> Result<()> {
    let masked = TemplateMasker::mask(source, |_| {}, None);
    let tree = crate::engine::languages::with_warm_parser("html", tree_sitter_html::language(), |html| {
        html.parse(masked.as_ref(), None)
    })?
    .context("Vue template parse cancelled")?;
    let owner = format!("module:{path}");
    let mut components = std::collections::HashMap::new();
    for alias in facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports")
        .filter(|reference| {
            let at = Position { line: reference.line, column: reference.column };
            setup.setup_positions.iter().any(|(start, end)| *start <= at && at < *end)
        })
        .filter_map(|reference| reference.alias.as_deref())
    {
        components.insert(alias.to_owned(), alias.to_owned());
        components.insert(kebab_case(alias), alias.to_owned());
    }
    for (name, alias) in setup.registered_components {
        components.insert(name.clone(), alias.clone());
        components.insert(kebab_case(name), alias.clone());
    }
    let mut document = Document {
        path,
        source,
        module,
        line_starts: std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
            )
            .collect(),
        scopes: std::collections::HashMap::from([(owner.clone(), module.to_owned())]),
        scope_bindings: std::collections::HashMap::new(),
        components,
        typed_iterables: setup.typed_iterables,
        flows,
    };
    crate::engine::languages::with_warm_parser("javascript", tree_sitter_javascript::language(), |javascript| {
        for element in tree
            .root_node()
            .named_children(&mut tree.root_node().walk())
        {
            if tag_name(element, source) == Some("template") {
                visit_element(element, &mut document, &owner, None, facts, javascript);
            }
        }
    })?;
    Ok(())
}

fn tag_name<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    let start = if node.kind() == "self_closing_tag" {
        node
    } else {
        node.named_child(0)?
    };
    if !matches!(start.kind(), "start_tag" | "self_closing_tag") {
        return None;
    }
    let name = start.named_child(0)?;
    (name.kind() == "tag_name").then(|| text(name, source))
}

fn attributes<'tree>(start: Syntax<'tree>) -> Vec<(Syntax<'tree>, Syntax<'tree>)> {
    let mut result = Vec::new();
    for attribute in start.named_children(&mut start.walk()) {
        if attribute.kind() != "attribute" {
            continue;
        }
        if let (Some(name), Some(mut value)) = (attribute.named_child(0), attribute.named_child(1))
        {
            if value.kind() == "quoted_attribute_value" {
                let Some(inner) = value.named_child(0) else {
                    continue;
                };
                value = inner;
            }
            result.push((name, value));
        }
    }
    result
}

fn scope(
    document: &mut Document<'_>,
    parent: &str,
    node: Syntax<'_>,
    bindings: Vec<String>,
    typed_bindings: Vec<(String, TypeExpr, SourcePosition)>,
    slot_provider: Option<(&str, &str)>,
    facts: &mut Facts,
) -> String {
    let path = document.path;
    let point = node.start_position();
    let id = format!("template_scope:{path}:{}:{}", point.row + 1, point.column);
    let parent_qual = document
        .scopes
        .get(parent)
        .map_or(document.module, String::as_str);
    let qualname = format!("{parent_qual}.template@{}:{}", point.row + 1, point.column);
    document.scopes.insert(id.clone(), qualname.clone());
    document.scope_bindings.insert(id.clone(), (parent.to_owned(), bindings.clone()));
    let mut scope_node = Node {
        id: id.clone(),
        kind: "template_scope".into(),
        name: format!("template@{}:{}", point.row + 1, point.column),
        qualname,
        path: path.into(),
        line: point.row + 1,
        end_line: node.end_position().row + 1,
        language: "vue".into(),
        is_test: is_test(path),
        generated: false,
        details: serde_json::json!({"bindings":bindings,"rebindings":[]}),
    };
    if let Some((component, slot)) = slot_provider {
        scope_node.details["vue_slot"] = serde_json::json!({"component": component, "slot": slot});
    }
    if !typed_bindings.is_empty() {
        let flow = ValueFlowFacts {
            bindings: typed_bindings.into_iter().map(|(name, type_expr, position)| BindingFact {
                name,
                position,
                value: ValueExpr::Annotated { type_expr },
                conditional: false,
            }).collect(),
            ..ValueFlowFacts::default()
        };
        if let Some(flows) = document.flows.as_deref_mut() {
            flows.insert(id.clone(), flow);
        } else {
            scope_node.details["value_flow"] = serde_json::to_value(flow).expect("valid Vue scope flow");
        }
    }
    facts.nodes.push(scope_node);
    facts.edges.push(Edge {
        src: parent.into(),
        dst: id.clone(),
        kind: "contains".into(),
        path: path.into(),
        line: point.row + 1,
        evidence: "Vue lexical template scope".into(),
        confidence: "exact".into(),
    });
    id
}

fn binding_names(pattern: &str, parser: &mut Parser) -> Vec<String> {
    let pattern = pattern.trim().trim_start_matches('(').trim_end_matches(')');
    let input = format!("let {pattern};");
    let Some(tree) = parser.parse(&input, None) else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }
    let Some(declaration) = tree.root_node().named_child(0) else {
        return Vec::new();
    };
    let mut stack: Vec<_> = declaration
        .named_children(&mut declaration.walk())
        .filter_map(|node| node.child_by_field_name("name"))
        .collect();
    let mut names = Vec::new();
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "identifier" | "shorthand_property_identifier_pattern"
        ) {
            names.push(text(node, &input).into());
        } else {
            stack.extend(node.named_children(&mut node.walk()));
        }
    }
    names.sort();
    names.dedup();
    names
}

fn first_loop_binding(pattern: &str, parser: &mut Parser) -> Option<String> {
    let pattern = pattern.trim().trim_start_matches('(').trim_end_matches(')');
    let input = format!("let {pattern};");
    let tree = parser.parse(&input, None)?;
    if tree.root_node().has_error() { return None; }
    let declaration = tree.root_node().named_child(0)?;
    let first = declaration.named_children(&mut declaration.walk())
        .find(|node| node.kind() == "variable_declarator")?;
    let name = first.child_by_field_name("name")
        .filter(|node| node.kind() == "identifier")?;
    Some(text(name, &input).to_owned())
}

fn template_binding_shadows(document: &Document<'_>, parent: &str, name: &str) -> bool {
    let mut scope = parent;
    for _ in 0..64 {
        let Some((next, bindings)) = document.scope_bindings.get(scope) else { return false; };
        if bindings.iter().any(|binding| binding == name) { return true; }
        scope = next;
    }
    true
}

fn visit_element(
    node: Syntax<'_>,
    document: &mut Document<'_>,
    parent: &str,
    parent_component: Option<&str>,
    facts: &mut Facts,
    parser: &mut Parser,
) {
    let source = document.source;
    let Some(start) = (if node.kind() == "self_closing_tag" {
        Some(node)
    } else {
        node.named_child(0)
    }) else {
        return;
    };
    let attrs = attributes(start);
    let mut bindings = Vec::new();
    let mut typed_bindings = Vec::new();
    let mut slot_provider = None;
    let mut slot_directives = 0;
    let tag = tag_name(node, source);
    for (name, value) in &attrs {
        let name = text(*name, source);
        let expression = text(*value, source);
        if name == "v-for" {
            if let Some((left, right)) = expression
                .split_once(" in ")
                .or_else(|| expression.split_once(" of "))
            {
                let loop_bindings = binding_names(left, parser);
                if let Some(element) = document.typed_iterables.get(right.trim())
                    .filter(|_| !template_binding_shadows(document, parent, right.trim())) {
                    if let Some(first) = first_loop_binding(left, parser).filter(|first| loop_bindings.contains(first)) {
                        let end = value.end_position();
                        typed_bindings.push((first, element.clone(), SourcePosition {
                            line: end.row + 1,
                            column: end.column,
                        }));
                    }
                }
                bindings.extend(loop_bindings);
                let relative = expression.len() - right.len();
                expression_facts(
                    right,
                    value.start_byte() + relative,
                    document,
                    parent,
                    false,
                    facts,
                    parser,
                );
            }
        } else if name == "v-slot" || name.starts_with("v-slot:") || name.starts_with('#') {
            bindings.extend(binding_names(expression, parser));
            slot_directives += 1;
            let slot = if name == "v-slot" { "default" } else if let Some(slot) = name.strip_prefix("v-slot:") { slot } else { &name[1..] };
            let component = if tag == Some("template") {
                parent_component
            } else {
                tag.and_then(|tag| document.components.get(tag).map(String::as_str))
            };
            if let Some(component) = component {
                if !slot.is_empty() && slot.len() <= 48 && component.len() <= 48 && slot_directives == 1 {
                    slot_provider = Some((component.to_owned(), slot.to_owned()));
                }
            }
        }
    }
    bindings.sort();
    bindings.dedup();
    if slot_directives != 1 { slot_provider = None; }
    let owner = if bindings.is_empty() && tag_name(node, source) != Some("template") {
        parent.to_owned()
    } else {
        scope(document, parent, node, bindings, typed_bindings,
            slot_provider.as_ref().map(|(component, slot)| (component.as_str(), slot.as_str())), facts)
    };
    let child_component = tag.and_then(|tag| document.components.get(tag)).cloned();
    if let Some(tag) = tag_name(node, source) {
        let tag_node = start.named_child(0).unwrap();
        if tag.bytes().next().is_some_and(|b| b.is_ascii_uppercase()) || tag.contains('-') {
            let point = tag_node.start_position();
            let imported_or_registered = document.components.contains_key(tag);
            let component = document
                .components
                .get(tag)
                .map_or(tag, String::as_str)
                .to_owned();
            facts.references.push(Reference {
                source: owner.clone(),
                expression: component,
                kind: "references".into(),
                line: point.row + 1,
                column: point.column,
                alias: None,
                module: None,
                dynamic: false,
                receiver_hint: (!imported_or_registered).then_some(ReceiverHint::VueComponentTag),
            });
        }
    }
    for (name, value) in attrs {
        let name = text(name, source);
        if name == "v-for"
            || name == "v-slot"
            || name.starts_with("v-slot:")
            || name.starts_with('#')
        {
            continue;
        }
        let event = name.starts_with('@') || name.starts_with("v-on:");
        if event || name.starts_with(':') || name.starts_with("v-") {
            expression_facts(
                text(value, source),
                value.start_byte(),
                document,
                &owner,
                event,
                facts,
                parser,
            );
        }
    }
    let mut content_offset = start.end_byte();
    for child in node.named_children(&mut node.walk()) {
        if child == start {
            continue;
        }
        if content_offset < child.start_byte() {
            interpolation_facts(
                &source[content_offset..child.start_byte()],
                content_offset,
                document,
                &owner,
                facts,
                parser,
            );
        }
        match child.kind() {
            "element" | "self_closing_tag" => visit_element(child, document, &owner, child_component.as_deref(), facts, parser),
            "text" => interpolation_facts(
                text(child, source),
                child.start_byte(),
                document,
                &owner,
                facts,
                parser,
            ),
            _ => {}
        }
        content_offset = content_offset.max(child.end_byte());
    }
    if content_offset < node.end_byte() {
        interpolation_facts(
            &source[content_offset..node.end_byte()],
            content_offset,
            document,
            &owner,
            facts,
            parser,
        );
    }
}

fn interpolation_facts(
    input: &str,
    absolute_start: usize,
    document: &mut Document<'_>,
    owner: &str,
    facts: &mut Facts,
    parser: &mut Parser,
) {
    let mut offset = 0;
    while let Some(open) = input[offset..].find("{{") {
        let begin = offset + open + 2;
        let Some(close) = input[begin..].find("}}") else {
            break;
        };
        expression_facts(
            &input[begin..begin + close],
            absolute_start + begin,
            document,
            owner,
            false,
            facts,
            parser,
        );
        offset = begin + close + 2;
    }
}

fn expression_facts(
    expression: &str,
    offset: usize,
    document: &mut Document<'_>,
    owner: &str,
    event: bool,
    facts: &mut Facts,
    parser: &mut Parser,
) {
    let source = document.source;
    let path = document.path;
    if expression.trim().is_empty() {
        return;
    }
    let point = document.point(offset);
    let Some(tree) = ast::parse_island(parser, expression, offset, point) else {
        return;
    };
    let row = point.row;
    let root = tree.root_node();
    if root.has_error() {
        facts.errors.push(Diagnostic {
            path: path.into(),
            line: row + 1,
            message: "unsupported Vue JavaScript template expression".into(),
        });
        return;
    }
    let bare_handler = event
        && root.named_child_count() == 1
        && root.named_child(0).is_some_and(|n| {
            n.kind() == "expression_statement"
                && n.named_child(0)
                    .is_some_and(|n| matches!(n.kind(), "identifier" | "member_expression"))
        });
    let shadows = std::collections::HashSet::new();
    let mut stack = vec![(root, owner.to_owned())];
    while let Some((node, current_owner)) = stack.pop() {
        let mut child_owner = current_owner.clone();
        if node.kind() == "arrow_function" {
            let parameters = node
                .child_by_field_name("parameters")
                .or_else(|| node.child_by_field_name("parameter"));
            let bindings = parameters
                .map(|n| binding_names(text(n, source), parser))
                .unwrap_or_default();
            child_owner = scope(document, &current_owner, node, bindings, Vec::new(), None, facts);
        }
        let ctx = SyntaxContext {
            node,
            source,
            owner: &child_owner,
            offset: 0,
            shadowed_require_scopes: &shadows,
            type_checking_aliases: None,
        };
        typescript::JAVASCRIPT.extract_calls(&ctx, facts);
        if node.kind() == "member_expression"
            && !node.parent().is_some_and(|p| {
                p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node)
            })
        {
            reference(
                node,
                source,
                &child_owner,
                if bare_handler { "calls" } else { "references" },
                facts,
            );
        } else if matches!(node.kind(), "identifier" | "shorthand_property_identifier") {
            let callee = node.parent().is_some_and(|p| {
                p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node)
            });
            let handler_member = bare_handler
                && node
                    .parent()
                    .is_some_and(|p| p.kind() == "member_expression");
            let parameter = node.parent().is_some_and(|p| {
                matches!(p.kind(), "formal_parameters" | "arrow_function")
                    && (p.child_by_field_name("parameter") == Some(node)
                        || p.kind() == "formal_parameters")
            });
            if !callee && !handler_member && !parameter && text(node, source) != "$event" {
                reference(
                    node,
                    source,
                    &child_owner,
                    if bare_handler { "calls" } else { "references" },
                    facts,
                );
            }
        }
        stack.extend(
            node.named_children(&mut node.walk())
                .map(|n| (n, child_owner.clone())),
        );
    }
}

fn kebab_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            result.push('-');
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

fn reference(node: Syntax<'_>, source: &str, owner: &str, kind: &str, facts: &mut Facts) {
    let point = node.start_position();
    let expression = text(node, source);
    facts.references.push(Reference {
        source: owner.into(),
        expression: expression.into(),
        kind: kind.into(),
        line: point.row + 1,
        column: point.column,
        alias: None,
        module: None,
        dynamic: !expression
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '?')),
        receiver_hint: None,
    });
}
