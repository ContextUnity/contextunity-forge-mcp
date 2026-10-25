use super::*;

struct RequestAttribute<'a> {
    value: &'a str,
    line: usize,
    end_line: usize,
}

pub(super) fn extract_request_attributes(
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
        value_dynamic_prefixes,
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
        let Some((priority, prefix)) = prefixes.iter().enumerate().find(|(_, prefix)| {
            name.get(..prefix.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
        }) else {
            continue;
        };
        let Some(attribute_key) = name.get(prefix.len()..) else {
            continue;
        };
        let key = methods
            .keys()
            .find(|key| attribute_key.eq_ignore_ascii_case(key))
            .map(String::as_str)
            .or_else(|| {
                contexts
                    .iter()
                    .find(|key| attribute_key.eq_ignore_ascii_case(key))
                    .map(String::as_str)
            })
            .or_else(|| {
                values_attribute
                    .as_deref()
                    .filter(|key| attribute_key.eq_ignore_ascii_case(key))
            });
        let Some(key) = key else {
            continue;
        };
        let primary = priority == 0;
        let value = attribute
            .named_child(1)
            .map(|node| text(node, source).trim_matches(['\'', '"']).trim())
            .unwrap_or("");
        if primary || !values.contains_key(key) {
            values.insert(
                key.to_owned(),
                RequestAttribute {
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
            framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                facts: vec![crate::core::semantic::FrameworkFact::RuleOrigin(origin)],
                ..Default::default()
            })),
            ..Default::default()
        }),
    );
    for key in contexts {
        if let Some(attribute) = values.get(key) {
            context.insert(key.into(), json!(ast::bounded_expression(attribute.value)));
        }
    }
    if let Some(attribute_name) = values_attribute.as_deref() {
        if let Some(attribute) = values.get(attribute_name) {
            let value = attribute.value;
            let status_key = format!("{attribute_name}_status");
            if value_dynamic_prefixes
                .iter()
                .any(|prefix| value.starts_with(prefix))
            {
                context.insert(status_key, json!("dynamic"));
            } else if let Ok(Value::Object(object)) = serde_json::from_str::<Value>(value) {
                context.insert(status_key, json!("static_json"));
                context.insert(format!("{attribute_name}_key_count"), json!(object.len()));
            } else {
                context.insert(status_key, json!("invalid_or_non_object_json"));
            }
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
        details.insert(
            "url_status".into(),
            json!(request_url_status(attribute.value)),
        );
        facts.pending_flow_nodes.push(facts.nodes.len());
        facts.nodes.push(super::source_node(
            path,
            "html",
            super::NodeFields {
                id: id.clone(),
                kind: request_node.clone(),
                name: format!("{} {}", verb, ast::bounded_expression(attribute.value)),
                qualname: format!("{module}.{namespace}.{}.{method}", tag.start_byte()),
                line: attribute.line,
                end_line: attribute.end_line,
                details: Value::Object(details),
            },
        ));
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
        facts.pending_flow_nodes.push(facts.nodes.len());
        facts.nodes.push(super::source_node(
            path,
            "html",
            super::NodeFields {
                id: id.clone(),
                kind: context_node.clone(),
                name: context_label.clone(),
                qualname: format!("{module}.{namespace}.{}.context", tag.start_byte()),
                line,
                end_line: tag.end_position().row + 1,
                details: Value::Object(context),
            },
        ));
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

pub(super) fn request_url_status(value: &str) -> &'static str {
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
