use super::*;
use super::template::{TemplateMasker, TemplateTagKind};
#[path = "html/javascript.rs"]
mod javascript;
use serde_json::{json, Map, Value};

const HTMX_ATTRIBUTES: &[&str] = &[
    "get", "post", "put", "patch", "delete", "trigger", "target", "swap", "include", "vals",
];

/// Represents html data.
pub struct Html;
/// Shared html language profile.
pub static HTML: Html = Html;
/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&HTML];

impl LanguageProfile for Html {
    fn id(&self) -> &'static str {
        "html"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        by_id("javascript").map_or(&[], |profile| profile.manifest_filenames())
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        by_id("javascript").map_or_else(Vec::new, |profile| {
            profile.extract_manifest_dependencies(filename, content)
        })
    }
    fn is_stdlib(&self, module: &str) -> bool {
        by_id("javascript").is_some_and(|profile| profile.is_stdlib(module))
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
            Path::new(value)
                .extension()
                .and_then(|extension| extension.to_str()),
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
        if tag_name.eq_ignore_ascii_case("script")
            && !attribute_value(ctx.node, ctx.source, "type")
                .is_some_and(|script_type| script_type.eq_ignore_ascii_case("module"))
            && !has_attribute(ctx.node, ctx.source, "async")
            && !has_attribute(ctx.node, ctx.source, "defer")
            && htmx_url_status(value) == "local_unverified"
        {
            if let Some(reference) = facts.references.last_mut() {
                reference.receiver_hint = Some(crate::core::models::ReceiverHint::HtmlClassicScriptSource);
            }
        }
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
    fn builtin(&self, name: &str) -> bool {
        if let Some(name) = name.strip_prefix("template.tag.") {
            return template_builtin("tag", name);
        }
        if let Some(name) = name.strip_prefix("template.filter.") {
            return template_builtin("filter", name);
        }
        by_id("javascript").is_some_and(|profile| profile.builtin(name))
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        extract_file_impl(path, source, module, facts, None)
    }

    fn finish(&self, facts: &mut Facts) {
        javascript::finish(facts);
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

fn template_builtin(kind: &str, name: &str) -> bool {
    match kind {
        "tag" => matches!(
            name,
            "url" | "static" | "trans" | "block" | "include" | "extends" | "csrf_token"
                | "load" | "import" | "from" | "translate" | "with" | "endwith"
                | "endblock" | "if" | "elif" | "else" | "endif" | "for" | "empty"
                | "endfor" | "macro" | "endmacro" | "set" | "autoescape"
                | "endautoescape" | "filter" | "endfilter"
        ),
        "filter" => matches!(
            name,
            "default" | "date" | "length" | "json_script" | "slugify" | "escape"
                | "escapejs" | "safe" | "upper" | "lower" | "urlencode" | "e"
                | "selectattr" | "list" | "first" | "last" | "join" | "linebreaks"
                | "linebreaksbr" | "truncatechars" | "truncatewords" | "striptags"
                | "floatformat" | "pluralize" | "tojson" | "int" | "float" | "round" | "sum"
        ),
        _ => false,
    }
}

fn emit_template_builtin(path: &str, kind: &str, name: &str, line: usize, jinja: bool, facts: &mut Facts) {
    facts.references.push(Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression: format!("template.{kind}.{name}"),
        kind: "references".into(),
        line,
        column: 0,
        alias: None,
        module: (jinja && ((kind == "filter" && matches!(name, "e" | "selectattr" | "list"))
            || (kind == "tag" && matches!(name, "macro" | "endmacro" | "set" | "import" | "from"))))
            .then(|| "jinja".into()),
        receiver_hint: None,
    });
}

fn emit_template_filters(content: &str, path: &str, line: usize, jinja: bool, facts: &mut Facts) {
    let bytes = content.as_bytes();
    let mut quote = None;
    let mut offset = 0;
    while offset < bytes.len() {
        match (quote, bytes[offset]) {
            (Some(active), byte) if byte == active => quote = None,
            (Some(_), b'\\') => offset = (offset + 1).min(bytes.len() - 1),
            (None, b'\'' | b'"') => quote = Some(bytes[offset]),
            (None, b'|') if bytes.get(offset + 1) != Some(&b'|') => {
                let mut start = offset + 1;
                while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
                    start += 1;
                }
                let mut end = start;
                while bytes.get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                    end += 1;
                }
                if let Some(name) = content.get(start..end).filter(|name| valid_template_name(name)) {
                    emit_template_builtin(path, "filter", name, line, jinja, facts);
                }
                offset = end.saturating_sub(1);
            }
            _ => {}
        }
        offset += 1;
    }
}

pub(crate) fn valid_template_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn emit_template_load(path: &str, library: &str, selected: Option<&str>, line: usize, facts: &mut Facts) {
    facts.references.push(Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression: library.to_owned(),
        kind: "template_loads".into(),
        line,
        column: 0,
        alias: selected.map(str::to_owned),
        module: Some(library.to_owned()),
        receiver_hint: None,
    });
}

fn emit_template_loads(content: &str, path: &str, line: usize, facts: &mut Facts) -> bool {
    let Some(rest) = content.strip_prefix("load ") else { return false; };
    let parts: Vec<_> = rest.split_whitespace().collect();
    if let Some(index) = parts.iter().position(|part| *part == "from") {
        if index == 0 || index + 2 != parts.len() || !valid_template_name(parts[index + 1]) {
            return false;
        }
        let library = parts[index + 1];
        if !parts[..index].iter().all(|name| valid_template_name(name)) {
            return false;
        }
        for name in &parts[..index] {
            emit_template_load(path, library, Some(name), line, facts);
        }
    } else {
        if parts.is_empty() || !parts.iter().all(|name| valid_template_name(name)) {
            return false;
        }
        for library in parts {
            emit_template_load(path, library, None, line, facts);
        }
    }
    true
}

fn emit_template_binding(path: &str, module: &str, name: &str, kind: &str, line: usize, facts: &mut Facts) {
    let id = format!("template:{path}:{line}:{name}");
    facts.nodes.push(Node {
        id: id.clone(),
        kind: kind.into(),
        name: name.into(),
        qualname: format!("{module}.{name}"),
        path: path.into(),
        line,
        end_line: line,
        is_test: crate::core::models::is_test(path),
        language: "html".into(),
        generated: false,
        details: json!({"template_binding": kind}),
    });
    facts.edges.push(crate::core::models::Edge {
        src: format!("module:{path}"),
        dst: id,
        kind: "contains".into(),
        path: path.into(),
        line,
        evidence: "template declaration".into(),
        confidence: "exact".into(),
    });
}

fn emit_template_import_binding(path: &str, target: &str, name: &str, alias: &str, line: usize, facts: &mut Facts) {
    facts.references.push(Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression: name.into(),
        kind: "template_bindings".into(),
        line,
        column: 0,
        alias: Some(alias.into()),
        module: Some(target.into()),
        receiver_hint: None,
    });
}

fn quoted_template_target(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let quote = input.chars().next().filter(|quote| matches!(quote, '\'' | '"'))?;
    let remaining = &input[1..];
    let end = remaining.find(quote)?;
    let target = remaining.get(..end).filter(|target| !target.is_empty())?;
    Some((target, &remaining[end + 1..]))
}

fn emit_template_import_bindings(content: &str, path: &str, line: usize, facts: &mut Facts) {
    if let Some(rest) = content.strip_prefix("import ") {
        if let Some((target, suffix)) = quoted_template_target(rest) {
            if let Some(alias) = suffix.trim().strip_prefix("as ").map(str::trim).filter(|alias| valid_template_name(alias)) {
                emit_template_import_binding(path, target, "*", alias, line, facts);
            }
        }
    } else if let Some(rest) = content.strip_prefix("from ") {
        if let Some((target, suffix)) = quoted_template_target(rest) {
            if let Some(imports) = suffix.trim().strip_prefix("import ") {
                for entry in imports.split(',') {
                    let mut words = entry.split_whitespace();
                    let Some(name) = words.next().filter(|name| valid_template_name(name)) else { continue; };
                    let alias = match words.next() {
                        None => name,
                        Some("as") => match words.next().filter(|alias| valid_template_name(alias)) {
                            Some(alias) if words.next().is_none() => alias,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    emit_template_import_binding(path, target, name, alias, line, facts);
                }
            }
        }
    }
}

fn emit_template_expression(path: &str, content: &str, line: usize, facts: &mut Facts) {
    let expression = content.trim();
    let expression = if valid_template_name(expression) {
        Some(format!("template.variable.{expression}"))
    } else if let Some((callee, arguments)) = expression.split_once('(') {
        let callee = callee.trim();
        let valid_callee = callee.split_once('.').map_or_else(
            || valid_template_name(callee),
            |(namespace, name)| valid_template_name(namespace) && valid_template_name(name),
        );
        (arguments.ends_with(')') && valid_callee)
            .then(|| format!("template.macro.{callee}"))
    } else {
        None
    };
    if let Some(expression) = expression {
        facts.references.push(Reference {
            source: format!("module:{path}"),
            dynamic: false,
            expression,
            kind: "references".into(),
            line,
            column: 0,
            alias: None,
            module: None,
            receiver_hint: None,
        });
    }
}

fn preprocess_template<'a>(source: &'a str, path: &str, module: &str, facts: &mut Facts) -> std::borrow::Cow<'a, str> {
    let mut has_loaded_library = false;
    let mut jinja_directive = false;
    let mut nested_scope = 0usize;
    TemplateMasker::mask(
        source,
        |tag| {
            if !tag.closed || tag.kind == TemplateTagKind::Comment {
                return;
            }
            let content = tag.content.trim();
            if tag.kind == TemplateTagKind::Statement {
                let directive = content.split_whitespace().next().unwrap_or("");
                jinja_directive |= matches!(directive, "macro" | "endmacro" | "set" | "import" | "from");
                if matches!(directive, "endfor" | "endif" | "endwith" | "endblock" | "endmacro" | "endcall" | "endfilter" | "endautoescape" | "endtrans" | "endraw" | "endset") {
                    nested_scope = nested_scope.saturating_sub(1);
                }
                if nested_scope == 0 {
                    if let Some(rest) = content.strip_prefix("set ") {
                        if let Some((name, value)) = rest.split_once('=') {
                            let name = name.trim();
                            if valid_template_name(name) && !value.trim().is_empty() {
                                emit_template_binding(path, module, name, "variable", tag.line, facts);
                            }
                        }
                    } else if let Some(rest) = content.strip_prefix("macro ") {
                        if let Some((name, arguments)) = rest.split_once('(') {
                            let name = name.trim();
                            if valid_template_name(name) && arguments.trim_end().ends_with(')') {
                                emit_template_binding(path, module, name, "function", tag.line, facts);
                            }
                        }
                    }
                    emit_template_import_bindings(content, path, tag.line, facts);
                }
                if let Some(name) = content.split_whitespace().next().filter(|name| valid_template_name(name)) {
                    if template_builtin("tag", name) || has_loaded_library {
                        emit_template_builtin(path, "tag", name, tag.line, jinja_directive, facts);
                    }
                }
                has_loaded_library |= emit_template_loads(content, path, tag.line, facts);
                if let Some(rest) = content.strip_prefix("include ") {
                    if let Some(target) = extract_template_target(rest) {
                        emit_template_reference(path, target, "includes", tag.line, facts);
                    }
                } else if let Some(rest) = content.strip_prefix("extends ") {
                    if let Some(target) = extract_template_target(rest) {
                        emit_template_reference(path, target, "extends", tag.line, facts);
                    }
                } else if let Some(rest) = content
                    .strip_prefix("import ")
                    .or_else(|| content.strip_prefix("from "))
                {
                    if let Some(target) = extract_template_target(rest) {
                        emit_template_reference(path, target, "template_imports", tag.line, facts);
                    }
                }
                if matches!(directive, "for" | "if" | "with" | "block" | "macro" | "call" | "filter" | "autoescape" | "trans" | "raw")
                    || (directive == "set" && !content.contains('='))
                {
                    nested_scope += 1;
                }
            } else if tag.kind == TemplateTagKind::Interpolation && nested_scope == 0 {
                emit_template_expression(path, content, tag.line, facts);
            }
            emit_template_filters(content, path, tag.line, jinja_directive, facts);
        },
        Some(|slice| !is_html_entity(slice)),
    )
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

pub(crate) fn local_template_target(owner: &str, target: &str) -> Option<String> {
    let target = target.strip_prefix("./").unwrap_or(target);
    if target.is_empty()
        || target.contains([':', '\\'])
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let template_root = if owner.starts_with("templates/") {
        "templates/"
    } else if let Some(index) = owner.rfind("/templates/") {
        &owner[..index + "/templates/".len()]
    } else {
        owner.rsplit_once('/').map_or("", |(parent, _)| parent)
    };
    if template_root.is_empty() {
        Some(target.to_owned())
    } else if template_root.ends_with('/') {
        Some(format!("{template_root}{target}"))
    } else {
        Some(format!("{template_root}/{target}"))
    }
}

fn emit_template_reference(path: &str, target: &str, kind: &str, line: usize, facts: &mut Facts) {
    facts.references.push(Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression: target.to_string(),
        kind: kind.into(),
        line,
        column: 0,
        alias: None,
        module: Some(target.to_string()),
        receiver_hint: None,
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

fn has_attribute(tag: Syntax<'_>, source: &str, name: &str) -> bool {
    let mut cursor = tag.walk();
    let present = tag.named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
        .any(|attribute| attribute.named_child(0)
            .is_some_and(|node| text(node, source).eq_ignore_ascii_case(name)));
    present
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

fn extract_file_impl(
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
    mut flows: Option<&mut crate::core::typed_facts::FlowStore>,
) -> Result<()> {
    let has_template_tags = source.contains("{%")
        || source.contains("{{")
        || source.contains("{#")
        || source.contains('&');
    let parsed_source = if has_template_tags {
        preprocess_template(source, path, module, facts)
    } else {
        std::borrow::Cow::Borrowed(source)
    };
    let parse_input = parsed_source.as_ref();
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
                    let file = javascript.prepare(tree.root_node(), &wrapped);
                    let mut nodes = vec![tree.root_node()];
                    while let Some(node) = nodes.pop() {
                        javascript.extract_imports(
                            &SyntaxContext {
                                node,
                                source: &wrapped,
                                owner: &owner,
                                offset: 0,
                                shadowed_require_scopes: &file.shadowed_require_scopes,
                                type_checking_aliases: Some(&file.type_checking_aliases),
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
    let tree = with_warm_parser("html", tree_sitter_html::language(), |parser| {
        parser.parse(parse_input, None)
    })?
    .context("HTML parse cancelled")?;
    let err_count = facts.errors.len();
    ast::extract_tree_with_flows(
        &HTML,
        tree.root_node(),
        path,
        source,
        module,
        &format!("module:{path}"),
        facts,
        flows.as_deref_mut(),
    );
    if has_template_tags {
        facts.errors.truncate(err_count);
    }
    let javascript_profile = by_id("javascript");
    let mut global_scope = None;
    if let Some(profile) = javascript_profile {
        with_warm_parser("javascript", tree_sitter_javascript::language(), |parser| {
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                if matches!(node.kind(), "start_tag" | "self_closing_tag") {
                    let tag_text = text(node, source);
                    if tag_text.contains("hx-") {
                        extract_htmx(node, source, path, module, facts);
                    }
                    if tag_text.contains(" on")
                        || tag_text.contains("\non")
                        || tag_text.contains("\ton")
                        || tag_text.contains('@')
                        || tag_text.contains("x-on")
                        || tag_text.contains("x-data")
                    {
                        javascript::extract_handlers(
                            node,
                            profile,
                            parser,
                            path,
                            source,
                            module,
                            &mut global_scope,
                            facts,
                            flows.as_deref_mut(),
                        )?;
                    }
                }
                if node.kind() == "script_element" && executable_script(node, source) {
                    let mut cursor = node.walk();
                    for body in node
                        .named_children(&mut cursor)
                        .filter(|n| n.kind() == "raw_text")
                    {
                        javascript::extract_island(
                            profile,
                            parser,
                            path,
                            source,
                            module,
                            body.start_byte(),
                            body.end_byte(),
                            body.start_position(),
                            javascript::script_kind(node, source),
                            &mut global_scope,
                            facts,
                            flows.as_deref_mut(),
                        )?;
                        let first_full_line = body.start_position().row
                            + if body.start_position().column == 0 { 1 } else { 2 };
                        let last_full_line = body.end_position().row;
                        let exclusive_inline = if node.start_position().row == node.end_position().row {
                            let line_start = source[..node.start_byte()].rfind('\n').map_or(0, |index| index + 1);
                            let line_end = source[node.end_byte()..]
                                .find('\n')
                                .map_or(source.len(), |index| node.end_byte() + index);
                            source[line_start..line_end].trim() == text(node, source).trim()
                        } else {
                            false
                        };
                        if first_full_line <= last_full_line || exclusive_inline {
                            let island_id = format!("html-js-island:{path}:{}", body.start_byte());
                            facts.nodes.push(Node {
                                id: island_id,
                                kind: "island_scope".into(),
                                name: "JavaScript script island".into(),
                                qualname: format!("{module}.script@{}", body.start_byte()),
                                path: path.into(),
                                line: if exclusive_inline { node.start_position().row + 1 } else { first_full_line },
                                end_line: if exclusive_inline { node.end_position().row + 1 } else { last_full_line },
                                is_test: crate::core::models::is_test(path),
                                language: "javascript".into(),
                                generated: false,
                                details: json!({}),
                            });
                        }
                    }
                    continue;
                }
                let mut cursor = node.walk();
                pending.extend(node.named_children(&mut cursor));
            }
            Ok::<(), anyhow::Error>(())
        })??;
    } else {
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            if matches!(node.kind(), "start_tag" | "self_closing_tag") {
                let tag_text = text(node, source);
                if tag_text.contains("hx-") {
                    extract_htmx(node, source, path, module, facts);
                }
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    Ok(())
}

pub(crate) fn extract_typed(
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
) -> Result<()> {
    extract_file_impl(
        path,
        source,
        module,
        &mut typed.facts,
        Some(&mut typed.flows),
    )?;
    javascript::finish_typed(&mut typed.facts, &mut typed.flows);
    Ok(())
}
