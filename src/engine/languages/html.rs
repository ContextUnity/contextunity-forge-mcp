use super::*;
use crate::core::models::Diagnostic;
use serde_json::{json, Map, Value};

const HTMX_ATTRIBUTES: &[&str] = &[
    "get", "post", "put", "patch", "delete", "trigger", "target", "swap", "include", "vals",
];

pub struct Html;
pub static HTML: Html = Html;
pub static PROFILES: &[&dyn LanguageProfile] = &[&HTML];

impl LanguageProfile for Html {
    fn id(&self) -> &'static str {
        "html"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["html", "htm"]
    }
    fn module_name(&self, path: &str) -> String {
        path.replace('/', ".")
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_html::language()
    }
    fn symbol_kind(&self, _kind: &str) -> Option<&'static str> {
        None
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "html"
    }
    fn pattern_wrapper(&self, kind: &str) -> bool {
        kind == "document"
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() != "start_tag" && ctx.node.kind() != "self_closing_tag" {
            return;
        }
        let Some(tag_name) = ctx.node.named_child(0).map(|node| text(node, ctx.source)) else {
            return;
        };
        let attribute = if tag_name.eq_ignore_ascii_case("script") {
            if !javascript_type(attribute_value(ctx.node, ctx.source, "type").unwrap_or("")) {
                return;
            }
            "src"
        } else if tag_name.eq_ignore_ascii_case("link") {
            "href"
        } else {
            return;
        };
        let Some(value) = attribute_value(ctx.node, ctx.source, attribute) else {
            return;
        };
        let value = value.split(['?', '#']).next().unwrap_or("");
        if value.is_empty() || value.contains(":") || value.starts_with("//") {
            return;
        }
        if !matches!(
            Path::new(value).extension().and_then(|extension| extension.to_str()),
            Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts")
        ) {
            return;
        }
        let module = if value.starts_with('.') || value.starts_with('/') {
            value.to_owned()
        } else {
            format!("./{value}")
        };
        ctx.import(facts, module.clone(), None, Some(module));
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        self.normalize_import_with_root(None, owner, module)
    }
    fn normalize_import_with_root(
        &self,
        root: Option<&Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        if module.starts_with('/') {
            return Some(ImportPath {
                namespace: module_stem(module.trim_start_matches('/')).replace('/', "."),
                relative: true,
                symbol_path: false,
            });
        }
        by_id("javascript")?.normalize_import_with_root(root, owner, module)
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        if module.starts_with("https://") || module.starts_with("http://") {
            Some("remote JavaScript module")
        } else {
            by_id("javascript").and_then(|profile| profile.external_import(module))
        }
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        let (parsed_source, _has_template_tags) = preprocess_template(source, path, facts);
        let parse_input = parsed_source.as_deref().unwrap_or(source);
        let is_script_fragment = (path.contains("_script") || path.contains("script"))
            && !parse_input.contains("<html")
            && !parse_input.contains("<div")
            && !parse_input.contains("<body")
            && !parse_input.contains("<template")
            && !parse_input.contains("<script");
        if is_script_fragment {
            if let Some(javascript) = by_id("javascript") {
                let mut wrapped = String::with_capacity(parse_input.len() + 8);
                wrapped.push_str("({\n");
                wrapped.push_str(parse_input);
                wrapped.push_str("\n})");
                if let Ok(mut parser) = javascript.create_parser(path) {
                    if let Some(tree) = parser.parse(&wrapped, None) {
                        let owner = format!("module:{path}");
                        let mut nodes = vec![tree.root_node()];
                        while let Some(node) = nodes.pop() {
                            javascript.extract_imports(
                                &SyntaxContext {
                                    node,
                                    source: &wrapped,
                                    owner: &owner,
                                    offset: 0,
                                },
                                facts,
                            );
                            let mut cursor = node.walk();
                            nodes.extend(node.named_children(&mut cursor));
                        }
                        return Ok(());
                    }
                }
            }
        }
        let tree = self
            .create_parser(path)?
            .parse(parse_input, None)
            .context("HTML parse cancelled")?;
        let err_count = facts.errors.len();
        ast::extract_tree(self, tree.root_node(), path, source, module, facts);
        if _has_template_tags {
            facts.errors.truncate(err_count);
        }
        let javascript = by_id("javascript");
        let mut parser = javascript
            .map(|profile| profile.create_parser(path))
            .transpose()?;
        let owner = format!("module:{path}");
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            if matches!(node.kind(), "start_tag" | "self_closing_tag") {
                extract_htmx(node, source, path, module, facts);
            }
            if node.kind() == "script_element" {
                if let (Some(javascript), Some(parser)) = (javascript, parser.as_mut()) {
                    if executable_script(node, source) {
                        let mut cursor = node.walk();
                        for body in node
                            .named_children(&mut cursor)
                            .filter(|n| n.kind() == "raw_text")
                        {
                            let script = text(body, source);
                            let has_script_templates = script.contains("{%")
                                || script.contains("{{");
                            let parsed = parser
                                .parse(script, None)
                                .context("embedded JavaScript parse cancelled")?;
                            let offset = body.start_position().row;
                            let mut script_nodes = vec![parsed.root_node()];
                            while let Some(script_node) = script_nodes.pop() {
                                if (script_node.is_error() || script_node.is_missing())
                                    && !has_script_templates
                                {
                                    facts.errors.push(Diagnostic {
                                        path: path.into(),
                                        line: offset + script_node.start_position().row + 1,
                                        message: "syntax error in embedded JavaScript".into(),
                                    });
                                }
                                javascript.extract_imports(
                                    &SyntaxContext {
                                        node: script_node,
                                        source: script,
                                        owner: &owner,
                                        offset,
                                    },
                                    facts,
                                );
                                let mut cursor = script_node.walk();
                                script_nodes.extend(script_node.named_children(&mut cursor));
                            }
                        }
                    }
                }
                continue;
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        Ok(())
    }
}

fn is_html_entity(slice: &str) -> bool {
    if !slice.starts_with('&') {
        return false;
    }
    if let Some(semi) = slice.find(';') {
        if semi > 1 && semi <= 12 {
            let inner = &slice[1..semi];
            if let Some(num) = inner.strip_prefix('#') {
                if let Some(hex) = num.strip_prefix(['x', 'X']) {
                    !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
                } else {
                    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
                }
            } else {
                inner.chars().all(|c| c.is_ascii_alphanumeric())
            }
        } else {
            false
        }
    } else {
        false
    }
}

fn preprocess_template(
    source: &str,
    path: &str,
    facts: &mut Facts,
) -> (Option<String>, bool) {
    let has_templates = source.contains("{%") || source.contains("{{") || source.contains("{#");
    let has_amp = source.contains('&');
    if !has_templates && !has_amp {
        return (None, false);
    }
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' {
            if bytes[i + 1] == b'#' {
                let start = i;
                let end = match source[start + 2..].find("#}") {
                    Some(rel) => start + 2 + rel + 2,
                    None => bytes.len(),
                };
                for b in &bytes[start..end] {
                    out.push(if *b == b'\n' { '\n' } else { ' ' });
                }
                i = end;
                continue;
            } else if bytes[i + 1] == b'%' {
                let start = i;
                let (end, closed) = match source[start + 2..].find("%}") {
                    Some(rel) => (start + 2 + rel + 2, true),
                    None => (bytes.len(), false),
                };
                if closed && end >= start + 4 {
                    let tag_content = source[start + 2..end - 2].trim();
                    let line = source[..start].bytes().filter(|b| *b == b'\n').count() + 1;

                    if let Some(rest) = tag_content.strip_prefix("include ") {
                        if let Some(target) = extract_template_target(rest) {
                            emit_template_reference(path, target, "includes", line, facts);
                        }
                    } else if let Some(rest) = tag_content.strip_prefix("extends ") {
                        if let Some(target) = extract_template_target(rest) {
                            emit_template_reference(path, target, "extends", line, facts);
                        }
                    }
                }

                for b in &bytes[start..end] {
                    out.push(if *b == b'\n' { '\n' } else { ' ' });
                }
                i = end;
                continue;
            } else if bytes[i + 1] == b'{' {
                let start = i;
                let end = match source[start + 2..].find("}}") {
                    Some(rel) => start + 2 + rel + 2,
                    None => bytes.len(),
                };
                for b in &bytes[start..end] {
                    out.push(if *b == b'\n' { '\n' } else { ' ' });
                }
                i = end;
                continue;
            }
        }
        if bytes[i] == b'&' && !is_html_entity(&source[i..]) {
            out.push(' ');
            i += 1;
            continue;
        }
        let c = source[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    (Some(out), true)
}

fn extract_template_target(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &trimmed[1..];
    let end = rest.find(quote)?;
    let target = &rest[..end];
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

fn emit_template_reference(
    path: &str,
    target: &str,
    kind: &str,
    line: usize,
    facts: &mut Facts,
) {
    facts.references.push(Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression: target.to_string(),
        kind: kind.into(),
        line,
        alias: None,
        module: Some(target.to_string()),
    });
}

struct HtmxAttribute<'a> {
    value: &'a str,
    line: usize,
    end_line: usize,
}

fn extract_htmx(tag: Syntax<'_>, source: &str, path: &str, module: &str, facts: &mut Facts) {
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

fn htmx_url_status(value: &str) -> &'static str {
    if value.is_empty() || value.starts_with('#') {
        "current_page"
    } else if value.starts_with("//") || value.starts_with("http://") || value.starts_with("https://") {
        "remote"
    } else if value.contains(':')
        || value.contains('\\')
        || value.chars().any(|c| c.is_whitespace() || "{}<>$".contains(c))
    {
        "dynamic_or_nonlocal"
    } else {
        "local_unverified"
    }
}

fn attribute_value<'a>(tag: Syntax<'_>, source: &'a str, name: &str) -> Option<&'a str> {
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
    {
        if attribute
            .named_child(0)
            .is_some_and(|node| text(node, source).eq_ignore_ascii_case(name))
        {
            return attribute
                .named_child(1)
                .map(|node| text(node, source).trim_matches(['\'', '"']).trim());
        }
    }
    None
}

fn executable_script(node: Syntax<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    let Some(tag) = node
        .named_children(&mut cursor)
        .find(|n| n.kind() == "start_tag")
    else {
        return false;
    };
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "attribute")
    {
        let Some(name) = attribute.named_child(0).map(|n| text(n, source)) else {
            continue;
        };
        if name.eq_ignore_ascii_case("src") {
            return false;
        }
        if name.eq_ignore_ascii_case("type") {
            let value = attribute
                .named_child(1)
                .map(|n| text(n, source))
                .unwrap_or("");
            let value = value.trim_matches(['\'', '"']).trim();
            if !javascript_type(value) {
                return false;
            }
        }
    }
    true
}

fn javascript_type(value: &str) -> bool {
    [
        "",
        "module",
        "text/javascript",
        "application/javascript",
        "text/ecmascript",
        "application/ecmascript",
    ]
    .iter()
    .any(|mime| value.eq_ignore_ascii_case(mime))
}
