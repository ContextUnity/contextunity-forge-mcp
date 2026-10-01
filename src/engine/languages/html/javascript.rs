use super::*;
use crate::core::models::{Diagnostic, Edge};

#[allow(clippy::too_many_arguments)]
pub(super) fn extract_island(
    profile: &dyn LanguageProfile,
    parser: &mut tree_sitter::Parser,
    path: &str,
    source: &str,
    module: &str,
    start: usize,
    end: usize,
    position: tree_sitter::Point,
    kind: &str,
    global_scope: &mut Option<usize>,
    facts: &mut Facts,
    mut flows: Option<&mut crate::core::typed_facts::FlowStore>,
) -> Result<()> {
    let original = &source[start..end];
    let script = mask_templates(original);
    if script.trim().is_empty() {
        return Ok(());
    }
    let tree = ast::parse_island(parser, script.as_ref(), start, position)
        .context("embedded JavaScript parse cancelled")?;
    let row = position.row;
    let column = position.column;
    let root = tree.root_node();
    let line = row + 1;
    let global_owner = format!("html-js:{path}:classic");
    let global_qualname = format!("{module}.classic");
    let global_index = ensure_global_scope(
        path,
        &global_owner,
        &global_qualname,
        position,
        root.end_position().row + 1,
        global_scope,
        facts,
    );
    let (owner, scope) = if kind == "classic" {
        (global_owner.clone(), global_qualname.clone())
    } else {
        (
            format!("html-js:{path}:{start}:{kind}"),
            format!("{global_qualname}.{kind}@{start}"),
        )
    };
    let destination = facts;
    let mut island = Facts::default();
    let mut island_flows = crate::core::typed_facts::FlowStore::default();
    let facts = &mut island;
    facts.nodes.push(Node {
        id: owner.clone(),
        kind: "template_scope".into(),
        name: format!("{kind}@{start}"),
        qualname: scope.clone(),
        path: path.into(),
        line,
        end_line: root.end_position().row + 1,
        is_test: crate::core::models::is_test(path),
        language: "javascript".into(),
        generated: false,
        details: json!({"column":column,"embedded_language":"javascript","lexical_boundary":true,"html_import_owner":format!("module:{path}")}),
    });
    if kind != "classic" {
        facts.edges.push(Edge {
            src: global_owner,
            dst: owner.clone(),
            kind: "contains".into(),
            path: path.into(),
            line,
            evidence: format!("embedded JavaScript {kind}"),
            confidence: "exact".into(),
        });
    }
    let errors = facts.errors.len();
    ast::extract_tree_with_flows(
        profile,
        root,
        path,
        source,
        &scope,
        &owner,
        facts,
        flows.as_ref().map(|_| &mut island_flows),
    );
    if matches!(script, std::borrow::Cow::Owned(_)) {
        facts.errors.truncate(errors);
    }
    for diagnostic in &mut facts.errors[errors..] {
        diagnostic.message = format!("embedded JavaScript: {}", diagnostic.message);
    }
    let mut require_imports = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let value = match node.kind() {
            "variable_declarator" => node.child_by_field_name("value"),
            "assignment_expression" => node.child_by_field_name("right"),
            "call_expression" => Some(node),
            _ => None,
        };
        if let Some(value) = value.filter(|value| {
            value.kind() == "call_expression"
                && value
                    .child_by_field_name("function")
                    .is_some_and(|callee| text(callee, source).trim() == "require")
        }) {
            let position = node.start_position();
            let call_position = value.start_position();
            require_imports.push(json!([
                position.row + 1,
                position.column,
                call_position.row + 1,
                call_position.column
            ]));
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    if !require_imports.is_empty() {
        facts.nodes[0].details["require_imports"] = Value::Array(require_imports);
    }
    if kind == "handler" {
        if let Some(scope) = facts.nodes.iter_mut().find(|node| node.id == owner) {
            let mut bindings: std::collections::BTreeSet<String> = scope.details["bindings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect();
            bindings.insert("event".into());
            scope.details["bindings"] = json!(bindings);
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
    if kind == "classic" {
        let fragment = facts.nodes.swap_remove(0);
        merge_global_details(
            &mut destination.nodes[global_index].details,
            fragment.details,
        );
        if let Some(flows) = flows.as_deref_mut() {
            if let Some(mut flow) = island_flows.take(&fragment.id) {
                let target = &mut destination.nodes[global_index];
                if let Some(previous) = flows.get_mut(&target.id) {
                    previous.bindings.append(&mut flow.bindings);
                    previous.fields.append(&mut flow.fields);
                } else {
                    target.details["value_flow"] = Value::Null;
                    flows.insert_classic(target.id.clone(), flow);
                }
            }
        }
    }
    destination.nodes.append(&mut facts.nodes);
    destination.edges.append(&mut facts.edges);
    destination.references.append(&mut facts.references);
    destination.docs.append(&mut facts.docs);
    destination.errors.append(&mut facts.errors);
    if let Some(flows) = flows {
        flows.append(&mut island_flows);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn extract_handlers(
    tag: Syntax<'_>,
    profile: &dyn LanguageProfile,
    parser: &mut tree_sitter::Parser,
    path: &str,
    source: &str,
    module: &str,
    global_scope: &mut Option<usize>,
    facts: &mut Facts,
    mut flows: Option<&mut crate::core::typed_facts::FlowStore>,
) -> Result<()> {
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
    {
        let Some(name) = attribute.named_child(0).map(|node| text(node, source)) else {
            continue;
        };
        if name.len() <= 2
            || !name
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
        {
            continue;
        }
        let Some(value) = attribute.named_child(1) else {
            continue;
        };
        let raw = text(value, source);
        let quoted = raw.starts_with(['\'', '"']);
        let start = value.start_byte() + usize::from(quoted);
        let end = value.end_byte().saturating_sub(usize::from(quoted));
        if end < start {
            continue;
        }
        if source[start..end]
            .match_indices('&')
            .any(|(offset, _)| super::is_html_entity(&source[start + offset..end]))
        {
            facts.errors.push(Diagnostic {
                path: path.into(),
                line: value.start_position().row + 1,
                message: "HTML-encoded JavaScript handler requires decoding".into(),
            });
            continue;
        }
        let mut position = value.start_position();
        position.column += usize::from(quoted);
        extract_island(
            profile,
            parser,
            path,
            source,
            module,
            start,
            end,
            position,
            "handler",
            global_scope,
            facts,
            flows.as_deref_mut(),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ensure_global_scope(
    path: &str,
    owner: &str,
    qualname: &str,
    position: tree_sitter::Point,
    end_line: usize,
    index: &mut Option<usize>,
    facts: &mut Facts,
) -> usize {
    if let Some(index) = *index {
        let node = &mut facts.nodes[index];
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
    let next = facts.nodes.len();
    facts.nodes.push(Node {
        id: owner.into(), kind:"template_scope".into(), name:"classic scripts".into(), qualname:qualname.into(),
        path:path.into(), line:position.row + 1, end_line, is_test:crate::core::models::is_test(path), language:"javascript".into(), generated:false,
        details:json!({"column":position.column,"embedded_language":"javascript","classic_global":true,"lexical_boundary":true,"html_import_owner":format!("module:{path}")}),
    });
    facts.edges.push(Edge {
        src: format!("module:{path}"),
        dst: owner.into(),
        kind: "contains".into(),
        path: path.into(),
        line: position.row + 1,
        evidence: "classic JavaScript environment".into(),
        confidence: "exact".into(),
    });
    *index = Some(next);
    next
}

fn merge_global_details(target: &mut Value, mut fragment: Value) {
    for key in ["bindings", "rebindings", "require_imports"] {
        if let Value::Array(mut values) = fragment[key].take() {
            if !target[key].is_array() {
                target[key] = json!([]);
            }
            target[key]
                .as_array_mut()
                .expect("array initialized")
                .append(&mut values);
        }
    }
    for key in ["bindings", "fields"] {
        if let Value::Array(mut values) = fragment["value_flow"][key].take() {
            if !target["value_flow"].is_object() {
                target["value_flow"] = json!({});
            }
            if !target["value_flow"][key].is_array() {
                target["value_flow"][key] = json!([]);
            }
            target["value_flow"][key]
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
            && node.details["bindings"].as_array().is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|binding| binding.as_str() == Some("require"))
            })
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
            if let Some(values) = node.details["value_flow"][key].as_array_mut() {
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

fn mask_templates(script: &str) -> std::borrow::Cow<'_, str> {
    if !["{%", "{{", "{#"]
        .iter()
        .any(|marker| script.contains(marker))
    {
        return std::borrow::Cow::Borrowed(script);
    }
    let mut bytes = script.as_bytes().to_vec();
    let mut offset = 0;
    while offset + 1 < bytes.len() {
        let close = match &bytes[offset..offset + 2] {
            b"{%" => "%}",
            b"{{" => "}}",
            b"{#" => "#}",
            _ => {
                offset += 1;
                continue;
            }
        };
        let end = script[offset + 2..]
            .find(close)
            .map_or(bytes.len(), |length| offset + 2 + length + 2);
        for byte in &mut bytes[offset..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        offset = end;
    }
    std::borrow::Cow::Owned(String::from_utf8(bytes).expect("template masking preserves UTF-8"))
}
