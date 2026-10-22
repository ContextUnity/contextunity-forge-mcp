use super::*;

struct HtmxAttribute<'a> {
    value: &'a str,
    line: usize,
    end_line: usize,
}

pub(super) fn extract_htmx(
    tag: Syntax<'_>,
    source: &str,
    path: &str,
    module: &str,
    facts: &mut Facts,
    rule: &manifests::syntax::SyntaxRule,
) {
    let manifests::syntax::SyntaxOperation::HtmlRequestAttributes {
        request_node,
        context_node,
        namespace,
        context_label,
        prefixes,
        methods,
        contexts,
        values: values_attribute,
    } = &rule.operation
    else {
        return;
    };
    let origin = crate::core::semantic::ExtractionRuleOrigin {
        pack: rule.pack.clone(),
        rule: rule.id.clone(),
    };
    let mut values = std::collections::BTreeMap::new();
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
    {
        let Some(name) = attribute.named_child(0).map(|node| text(node, source)) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let Some((priority, key)) = prefixes
            .iter()
            .enumerate()
            .find_map(|(priority, prefix)| lower.strip_prefix(prefix).map(|key| (priority, key)))
        else {
            continue;
        };
        let primary = priority == 0;
        if !methods.contains_key(key)
            && !contexts.iter().any(|context| context == key)
            && key != values_attribute
        {
            continue;
        }
        let value = attribute
            .named_child(1)
            .map(|node| text(node, source).trim_matches(['\'', '"']).trim())
            .unwrap_or("");
        if primary || !values.contains_key(key) {
            values.insert(
                key.to_owned(),
                HtmxAttribute {
                    value,
                    line: attribute.start_position().row + 1,
                    end_line: attribute.end_position().row + 1,
                },
            );
        }
    }
    if values.is_empty() {
        return;
    }
    let mut context = Map::new();
    context.insert(
        "value_flow".into(),
        json!(crate::core::semantic::ValueFlowFacts {
            rule_origins: vec![origin],
            ..Default::default()
        }),
    );
    for key in contexts {
        if let Some(attribute) = values.get(key) {
            context.insert(key.into(), json!(ast::bounded_expression(attribute.value)));
        }
    }
    if let Some(attribute) = values.get(values_attribute) {
        let value = attribute.value;
        if value.starts_with("js:") || value.starts_with("javascript:") {
            context.insert("vals_status".into(), json!("dynamic"));
        } else if let Ok(Value::Object(object)) = serde_json::from_str::<Value>(value) {
            context.insert("vals_status".into(), json!("static_json"));
            context.insert("vals_key_count".into(), json!(object.len()));
        } else {
            context.insert("vals_status".into(), json!("invalid_or_non_object_json"));
        }
    }
    let mut found_request = false;
    for (method, verb) in methods {
        let Some(attribute) = values.get(method) else {
            continue;
        };
        found_request = true;
        let id = format!("{namespace}:{path}:{}:{method}", tag.start_byte());
        let mut details = context.clone();
        details.insert("url_status".into(), json!(htmx_url_status(attribute.value)));
        facts.nodes.push(Node {
            id: id.clone(),
            kind: request_node.clone(),
            name: format!("{} {}", verb, ast::bounded_expression(attribute.value)),
            qualname: format!("{module}.{namespace}.{}.{method}", tag.start_byte()),
            path: path.into(),
            line: attribute.line,
            end_line: attribute.end_line,
            is_test: crate::core::models::is_test(path),
            language: "html".into(),
            generated: false,
            details: Value::Object(details),
        });
        facts.edges.push(crate::core::models::Edge {
            src: format!("module:{path}"),
            dst: id,
            kind: "contains".into(),
            path: path.into(),
            line: attribute.line,
            evidence: format!("{}{method}", prefixes[0]),
            confidence: "exact".into(),
        });
    }
    if !found_request {
        let id = format!("{namespace}:{path}:{}:context", tag.start_byte());
        let line = tag.start_position().row + 1;
        facts.nodes.push(Node {
            id: id.clone(),
            kind: context_node.clone(),
            name: context_label.clone(),
            qualname: format!("{module}.{namespace}.{}.context", tag.start_byte()),
            path: path.into(),
            line,
            end_line: tag.end_position().row + 1,
            is_test: crate::core::models::is_test(path),
            language: "html".into(),
            generated: false,
            details: Value::Object(context),
        });
        facts.edges.push(crate::core::models::Edge {
            src: format!("module:{path}"),
            dst: id,
            kind: "contains".into(),
            path: path.into(),
            line,
            evidence: format!("{namespace} attributes"),
            confidence: "exact".into(),
        });
    }
}

pub(super) fn htmx_url_status(value: &str) -> &'static str {
    if value.is_empty() || value.starts_with('#') {
        "current_page"
    } else if value.starts_with("//")
        || value.starts_with("http://")
        || value.starts_with("https://")
    {
        "remote"
    } else if value.contains(':')
        || value.contains('\\')
        || value
            .chars()
            .any(|c| c.is_whitespace() || "{}<>$".contains(c))
    {
        "dynamic_or_nonlocal"
    } else {
        "local_unverified"
    }
}
