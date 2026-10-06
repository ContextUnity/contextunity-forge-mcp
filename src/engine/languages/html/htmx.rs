use super::*;

const HTMX_ATTRIBUTES: &[&str] = &[
    "get", "post", "put", "patch", "delete", "trigger", "target", "swap", "include", "vals",
];

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
) {
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
        let (key, primary) = if let Some(key) = lower.strip_prefix("hx-") {
            (key, true)
        } else if let Some(key) = lower.strip_prefix("data-hx-") {
            (key, false)
        } else {
            continue;
        };
        if !HTMX_ATTRIBUTES.contains(&key) {
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
    for key in ["trigger", "target", "swap", "include"] {
        if let Some(attribute) = values.get(key) {
            context.insert(key.into(), json!(ast::bounded_expression(attribute.value)));
        }
    }
    if let Some(attribute) = values.get("vals") {
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
    for method in ["get", "post", "put", "patch", "delete"] {
        let Some(attribute) = values.get(method) else {
            continue;
        };
        found_request = true;
        let id = format!("htmx:{path}:{}:{method}", tag.start_byte());
        let mut details = context.clone();
        details.insert("url_status".into(), json!(htmx_url_status(attribute.value)));
        facts.nodes.push(Node {
            id: id.clone(),
            kind: "htmx_request".into(),
            name: format!(
                "{} {}",
                method.to_ascii_uppercase(),
                ast::bounded_expression(attribute.value)
            ),
            qualname: format!("{module}.htmx.{}.{method}", tag.start_byte()),
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
            evidence: format!("hx-{method}"),
            confidence: "exact".into(),
        });
    }
    if !found_request {
        let id = format!("htmx:{path}:{}:context", tag.start_byte());
        let line = tag.start_position().row + 1;
        facts.nodes.push(Node {
            id: id.clone(),
            kind: "htmx_context".into(),
            name: "HTMX context".into(),
            qualname: format!("{module}.htmx.{}.context", tag.start_byte()),
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
            evidence: "htmx attributes".into(),
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
