use super::template::{TemplateMasker, TemplateTagKind};
use super::*;
mod javascript;
mod request_attributes;
use request_attributes::{extract_request_attributes, request_url_status};
use serde_json::{json, Map, Value};

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
    fn ast_search_capabilities(&self) -> AstSearchCapabilities {
        AstSearchCapabilities {
            attribute_kinds: &["attribute"],
            valid_examples: &[
                "<main><button disabled></button></main>",
                "<a href=\"#\">link</a>",
            ],
            diagnostic_hint: "Use a complete HTML element or document pattern.",
            ..AstSearchCapabilities::default()
        }
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
            && request_url_status(value) == "local_unverified"
        {
            if let Some(reference) = facts.references.last_mut() {
                reference.receiver_hint =
                    Some(crate::core::models::ReceiverHint::HtmlClassicScriptSource);
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
        by_id("javascript").is_some_and(|profile| profile.builtin(name))
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        extract_file_impl(
            path,
            source,
            module,
            facts,
            None,
            &dependency_registry::DependencyRegistry::bundled_framework_rules()
                .language_context_for_path(path),
        )
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

fn template_reference(path: &str, expression: String, kind: &str, line: usize) -> Reference {
    Reference {
        source: format!("module:{path}"),
        dynamic: false,
        expression,
        kind: kind.into(),
        line,
        column: 0,
        alias: None,
        module: None,
        receiver_hint: None,
    }
}

pub(super) struct NodeFields {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) qualname: String,
    pub(super) line: usize,
    pub(super) end_line: usize,
    pub(super) details: Value,
}

pub(super) fn source_node(path: &str, language: &'static str, fields: NodeFields) -> Node {
    Node {
        native_semantic: None,
        id: fields.id,
        kind: fields.kind,
        name: fields.name,
        qualname: fields.qualname,
        path: path.into(),
        line: fields.line,
        end_line: fields.end_line,
        is_test: crate::core::models::is_test(path),
        language: language.into(),
        generated: false,
        details: fields.details,
    }
}

fn emit_template_rule_reference(
    path: &str,
    kind: &str,
    name: &str,
    line: usize,
    facts: &mut Facts,
) {
    facts.references.push(template_reference(
        path,
        format!("template.{kind}.{name}"),
        "references",
        line,
    ));
}

fn named_route_tag<'a>(content: &'a str, tag: &str) -> Option<&'a str> {
    let rest = content.strip_prefix(tag)?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let quote = rest
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))?;
    let end = rest[quote.len_utf8()..].find(quote)? + quote.len_utf8();
    let name = &rest[quote.len_utf8()..end];
    if name.is_empty()
        || name.contains('\\')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return None;
    }
    Some(name)
}

fn emit_named_route_reference(
    path: &str,
    name: &str,
    line: usize,
    facts: &mut Facts,
    rule: &manifests::syntax::SyntaxRule,
    expression_prefix: &str,
) {
    facts.references.push(Reference {
        receiver_hint: Some(crate::core::models::ReceiverHint::NamedRoute {
            origin: crate::core::semantic::ExtractionRuleOrigin {
                pack: rule.pack.clone(),
                rule: rule.id.clone(),
            },
            key: name.to_owned(),
        }),
        ..template_reference(
            path,
            format!("{expression_prefix}{name}"),
            "references_route",
            line,
        )
    });
}

fn emit_template_filters(content: &str, path: &str, line: usize, facts: &mut Facts) {
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
                while bytes
                    .get(end)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    end += 1;
                }
                if let Some(name) = content
                    .get(start..end)
                    .filter(|name| valid_template_name(name))
                {
                    emit_template_rule_reference(path, "filter", name, line, facts);
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
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn emit_template_load(
    path: &str,
    library: &str,
    selected: Option<&str>,
    line: usize,
    facts: &mut Facts,
) {
    facts.references.push(Reference {
        alias: selected.map(str::to_owned),
        module: Some(library.to_owned()),
        ..template_reference(path, library.to_owned(), "template_loads", line)
    });
}

fn emit_template_loads(
    content: &str,
    path: &str,
    line: usize,
    facts: &mut Facts,
    dialect: &manifests::syntax::TemplateDialect,
) -> bool {
    let Some(rest) = after_directive(content, &dialect.load) else {
        return false;
    };
    let mut word_count = 0;
    let mut from_index = None;
    for (index, part) in rest.split_whitespace().enumerate() {
        if part == dialect.from && from_index.is_none() {
            from_index = Some(index);
        }
        word_count += 1;
    }
    if let Some(index) = from_index {
        if index == 0 || index + 2 != word_count {
            return false;
        }
        let Some(library) = rest.split_whitespace().nth(index + 1) else {
            return false;
        };
        if !valid_template_name(library)
            || !rest.split_whitespace().take(index).all(valid_template_name)
        {
            return false;
        }
        for name in rest.split_whitespace().take(index) {
            emit_template_load(path, library, Some(name), line, facts);
        }
    } else {
        if word_count == 0 || !rest.split_whitespace().all(valid_template_name) {
            return false;
        }
        for library in rest.split_whitespace() {
            emit_template_load(path, library, None, line, facts);
        }
    }
    true
}

fn emit_template_binding(
    path: &str,
    module: &str,
    name: &str,
    kind: &str,
    line: usize,
    facts: &mut Facts,
) {
    let id = format!("template:{path}:{line}:{name}");
    facts.nodes.push(source_node(
        path,
        "html",
        NodeFields {
            id: id.clone(),
            kind: kind.into(),
            name: name.into(),
            qualname: format!("{module}.{name}"),
            line,
            end_line: line,
            details: json!({"template_binding": kind}),
        },
    ));
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

fn emit_template_import_binding(
    path: &str,
    target: &str,
    name: &str,
    alias: &str,
    line: usize,
    facts: &mut Facts,
) {
    facts.references.push(Reference {
        alias: Some(alias.into()),
        module: Some(target.into()),
        ..template_reference(path, name.into(), "template_bindings", line)
    });
}

fn quoted_template_target(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let quote = input
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))?;
    let remaining = &input[1..];
    let end = remaining.find(quote)?;
    let target = remaining.get(..end).filter(|target| !target.is_empty())?;
    Some((target, &remaining[end + 1..]))
}

fn emit_template_import_bindings(
    content: &str,
    path: &str,
    line: usize,
    facts: &mut Facts,
    dialect: &manifests::syntax::TemplateDialect,
) {
    if let Some(rest) = after_directive(content, &dialect.import) {
        if let Some((target, suffix)) = quoted_template_target(rest) {
            if let Some(alias) = suffix
                .trim()
                .strip_prefix(dialect.alias.as_str())
                .and_then(|rest| rest.strip_prefix(' '))
                .map(str::trim)
                .filter(|alias| valid_template_name(alias))
            {
                emit_template_import_binding(path, target, "*", alias, line, facts);
            }
        }
    } else if let Some(rest) = after_directive(content, &dialect.from) {
        if let Some((target, suffix)) = quoted_template_target(rest) {
            if let Some(imports) = after_directive(suffix.trim(), &dialect.import) {
                for entry in imports.split(',') {
                    let mut words = entry.split_whitespace();
                    let Some(name) = words.next().filter(|name| valid_template_name(name)) else {
                        continue;
                    };
                    let alias = match words.next() {
                        None => name,
                        Some(keyword) if keyword == dialect.alias => {
                            match words.next().filter(|alias| valid_template_name(alias)) {
                                Some(alias) if words.next().is_none() => alias,
                                _ => continue,
                            }
                        }
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
        (arguments.ends_with(')') && valid_callee).then(|| format!("template.macro.{callee}"))
    } else {
        None
    };
    if let Some(expression) = expression {
        facts
            .references
            .push(template_reference(path, expression, "references", line));
    }
}

fn after_directive<'a>(content: &'a str, configured: &str) -> Option<&'a str> {
    let rest = content.strip_prefix(configured)?;
    rest.chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| rest.trim_start())
}

fn directory_matches_path(path: &str, directory: &str) -> bool {
    path.strip_prefix(directory)
        .is_some_and(|suffix| suffix.starts_with('/'))
        || path.match_indices(directory).any(|(index, _)| {
            index > 0
                && path.as_bytes().get(index - 1) == Some(&b'/')
                && path.as_bytes().get(index + directory.len()) == Some(&b'/')
        })
}

fn preprocess_template<'a>(
    source: &'a str,
    path: &str,
    module: &str,
    facts: &mut Facts,
    rules: &LanguageContext,
) -> std::borrow::Cow<'a, str> {
    let dialects: Vec<_> = rules
        .syntax_rules
        .iter()
        .filter_map(|rule| {
            let manifests::syntax::SyntaxOperation::TemplateDialect { dialect } = &rule.operation
            else {
                return None;
            };
            Some((rule, dialect))
        })
        .collect();
    let owned: Vec<_> = dialects
        .iter()
        .copied()
        .filter(|(_, dialect)| directory_matches_path(path, &dialect.directory))
        .collect();
    let mut nested_scope = 0usize;
    TemplateMasker::mask(
        source,
        |tag| {
            if !tag.closed || tag.kind == TemplateTagKind::Comment {
                return;
            }
            let first_reference = facts.references.len();
            let first_node = facts.nodes.len();
            let content = tag.content.trim();
            let directive = content.split_whitespace().next().unwrap_or("");
            let dialect = if !owned.is_empty() {
                (owned.len() == 1).then(|| owned[0])
            } else if tag.kind == TemplateTagKind::Statement {
                let mut matching = dialects.iter().copied().filter(|(_, dialect)| {
                    [
                        &dialect.assignment,
                        &dialect.function,
                        &dialect.import,
                        &dialect.from,
                        &dialect.load,
                        &dialect.include,
                        &dialect.extends,
                    ]
                    .iter()
                    .any(|name| name.as_str() == directive)
                        || dialect
                            .scope_open
                            .iter()
                            .chain(&dialect.scope_close)
                            .any(|name| name == directive)
                });
                if let Some(candidate) = matching.next() {
                    matching.next().is_none().then_some(candidate)
                } else {
                    (dialects.len() == 1).then(|| dialects[0])
                }
            } else {
                (dialects.len() == 1).then(|| dialects[0])
            };
            if tag.kind == TemplateTagKind::Statement {
                for rule in &rules.syntax_rules {
                    let manifests::syntax::SyntaxOperation::TemplateNamedRoute {
                        tag: configured,
                        expression_prefix,
                    } = &rule.operation
                    else {
                        continue;
                    };
                    if let Some(name) = named_route_tag(content, configured) {
                        emit_named_route_reference(
                            path,
                            name,
                            tag.line,
                            facts,
                            rule,
                            expression_prefix,
                        );
                    }
                }
                let Some((_, dialect)) = dialect else { return };
                let directive = content.split_whitespace().next().unwrap_or("");
                if dialect.scope_close.iter().any(|name| name == directive) {
                    nested_scope = nested_scope.saturating_sub(1);
                }
                if nested_scope == 0 {
                    if let Some(rest) = after_directive(content, &dialect.assignment) {
                        if let Some((name, value)) = rest.split_once('=') {
                            let name = name.trim();
                            if valid_template_name(name) && !value.trim().is_empty() {
                                emit_template_binding(
                                    path, module, name, "variable", tag.line, facts,
                                );
                            }
                        }
                    } else if let Some(rest) = after_directive(content, &dialect.function) {
                        if let Some((name, arguments)) = rest.split_once('(') {
                            let name = name.trim();
                            if valid_template_name(name) && arguments.trim_end().ends_with(')') {
                                emit_template_binding(
                                    path, module, name, "function", tag.line, facts,
                                );
                            }
                        }
                    }
                    emit_template_import_bindings(content, path, tag.line, facts, dialect);
                }
                if valid_template_name(directive) {
                    emit_template_rule_reference(path, "tag", directive, tag.line, facts);
                }
                emit_template_loads(content, path, tag.line, facts, dialect);
                for (name, relation) in [
                    (&dialect.include, "includes"),
                    (&dialect.extends, "extends"),
                    (&dialect.import, "template_imports"),
                    (&dialect.from, "template_imports"),
                ] {
                    if let Some(target) =
                        after_directive(content, name).and_then(extract_template_target)
                    {
                        emit_template_reference(path, target, relation, tag.line, facts);
                    }
                }
                if dialect.scope_open.iter().any(|name| name == directive)
                    || (directive == dialect.assignment && !content.contains('='))
                {
                    nested_scope += 1;
                }
            } else if dialect.is_some()
                && tag.kind == TemplateTagKind::Interpolation
                && nested_scope == 0
            {
                emit_template_expression(path, content, tag.line, facts);
            }
            if dialect.is_some() {
                emit_template_filters(content, path, tag.line, facts);
            }
            if let Some((rule, _)) = dialect {
                let origin = crate::core::semantic::ExtractionRuleOrigin {
                    pack: rule.pack.clone(),
                    rule: rule.id.clone(),
                };
                for reference in &mut facts.references[first_reference..] {
                    if reference.receiver_hint.is_none() {
                        reference.receiver_hint =
                            Some(crate::core::models::ReceiverHint::ConfiguredReference {
                                origin: origin.clone(),
                            });
                    }
                }
                facts
                    .pending_flow_nodes
                    .extend(first_node..facts.nodes.len());
                for node in &mut facts.nodes[first_node..] {
                    node.details["value_flow"] = json!(crate::core::semantic::ValueFlowFacts {
                        framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                            facts: vec![crate::core::semantic::FrameworkFact::RuleOrigin(
                                origin.clone()
                            )],
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
            }
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

pub(crate) fn local_template_target(
    owner: &str,
    target: &str,
    rules: &LanguageContext,
) -> Option<String> {
    let target = target.strip_prefix("./").unwrap_or(target);
    if target.is_empty()
        || target.contains([':', '\\'])
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let mut roots = std::collections::BTreeSet::new();
    for rule in &rules.syntax_rules {
        let manifests::syntax::SyntaxOperation::TemplateDialect { dialect } = &rule.operation
        else {
            continue;
        };
        let prefix = format!("{}/", dialect.directory);
        let middle = format!("/{prefix}");
        if owner.starts_with(&prefix) {
            roots.insert(prefix);
        } else if let Some(index) = owner.rfind(&middle) {
            roots.insert(owner[..index + middle.len()].to_owned());
        }
    }
    if roots.len() > 1 {
        return None;
    }
    let root = roots.into_iter().next();
    let template_root = root
        .as_deref()
        .unwrap_or_else(|| owner.rsplit_once('/').map_or("", |(parent, _)| parent));
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
        module: Some(target.to_owned()),
        ..template_reference(path, target.to_owned(), kind, line)
    });
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
    let present = tag
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "attribute")
        .any(|attribute| {
            attribute
                .named_child(0)
                .is_some_and(|node| text(node, source).eq_ignore_ascii_case(name))
        });
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
    rules: &Arc<LanguageContext>,
) -> Result<()> {
    let has_template_tags = source.contains("{%")
        || source.contains("{{")
        || source.contains("{#")
        || source.contains('&');
    let parsed_source = if has_template_tags {
        preprocess_template(source, path, module, facts, rules)
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
            if let Ok(Some(tree)) =
                with_warm_parser(javascript.id(), javascript.grammar(path), |parser| {
                    parser.parse(&wrapped, None)
                })
            {
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
    let tree = with_warm_parser("html", tree_sitter_html::language(), |parser| {
        parser.parse(parse_input, None)
    })?
    .context("HTML parse cancelled")?;
    let err_count = facts.errors.len();
    ast::extract_tree_with_language_context(
        &HTML,
        tree.root_node(),
        path,
        source,
        module,
        &format!("module:{path}"),
        facts,
        flows.as_deref_mut(),
        rules,
    );
    if has_template_tags {
        facts.errors.truncate(err_count);
    }
    let javascript_profile = by_id("javascript");
    let mut global_scope = None;
    if let Some(profile) = javascript_profile {
        with_warm_parser("javascript", tree_sitter_javascript::language(), |parser| {
            let mut context = javascript::ScriptExtraction {
                profile,
                parser,
                path,
                source,
                module,
                global_scope: &mut global_scope,
                facts,
                flows,
                rules,
            };
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                if matches!(node.kind(), "start_tag" | "self_closing_tag") {
                    for rule in &rules.syntax_rules {
                        if matches!(
                            rule.operation,
                            manifests::syntax::SyntaxOperation::HtmlRequestAttributes { .. }
                        ) {
                            extract_request_attributes(
                                node,
                                source,
                                path,
                                module,
                                context.facts,
                                rule,
                            );
                        }
                    }
                    javascript::extract_handlers(&mut context, node)?;
                }
                if node.kind() == "script_element" && executable_script(node, source) {
                    let mut cursor = node.walk();
                    for body in node
                        .named_children(&mut cursor)
                        .filter(|n| n.kind() == "raw_text")
                    {
                        javascript::extract_island(
                            &mut context,
                            javascript::Island {
                                start: body.start_byte(),
                                end: body.end_byte(),
                                position: body.start_position(),
                                kind: javascript::script_kind(node, source),
                            },
                        )?;
                        let first_full_line = body.start_position().row
                            + if body.start_position().column == 0 {
                                1
                            } else {
                                2
                            };
                        let last_full_line = body.end_position().row;
                        let exclusive_inline =
                            if node.start_position().row == node.end_position().row {
                                let line_start = source[..node.start_byte()]
                                    .rfind('\n')
                                    .map_or(0, |index| index + 1);
                                let line_end = source[node.end_byte()..]
                                    .find('\n')
                                    .map_or(source.len(), |index| node.end_byte() + index);
                                source[line_start..line_end].trim() == text(node, source).trim()
                            } else {
                                false
                            };
                        if first_full_line <= last_full_line || exclusive_inline {
                            let island_id = format!("html-js-island:{path}:{}", body.start_byte());
                            context.facts.nodes.push(source_node(
                                path,
                                "javascript",
                                NodeFields {
                                    id: island_id,
                                    kind: "island_scope".into(),
                                    name: "JavaScript script island".into(),
                                    qualname: format!("{module}.script@{}", body.start_byte()),
                                    line: if exclusive_inline {
                                        node.start_position().row + 1
                                    } else {
                                        first_full_line
                                    },
                                    end_line: if exclusive_inline {
                                        node.end_position().row + 1
                                    } else {
                                        last_full_line
                                    },
                                    details: Value::Null,
                                },
                            ));
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
                {
                    for rule in &rules.syntax_rules {
                        if matches!(
                            rule.operation,
                            manifests::syntax::SyntaxOperation::HtmlRequestAttributes { .. }
                        ) {
                            extract_request_attributes(node, source, path, module, facts, rule);
                        }
                    }
                }
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    // Reuse the already parsed filter occurrences for compact cached-provider
    // selection. These selectors are internal; the original reference remains
    // the sole owner of its edge and coverage row.
    let mut selectors = Vec::new();
    for rule in rules
        .syntax_rules
        .iter()
        .filter(|rule| rule.language == "html")
    {
        let manifests::syntax::SyntaxOperation::NamedBinding {
            domain,
            binding: manifests::projections::NamedBindingForm::TemplateReference { .. },
        } = &rule.operation
        else {
            continue;
        };
        for reference in facts
            .references
            .iter()
            .filter(|reference| reference.kind == "references")
        {
            let Some(key) = reference.expression.strip_prefix("template.filter.") else {
                continue;
            };
            let mut selector = reference.clone();
            selector.expression = key.to_owned();
            selector.kind = "semantic_named_reference".into();
            selector.module = Some(domain.clone());
            selector.receiver_hint = None;
            selectors.push(selector);
        }
    }
    facts.references.extend(selectors);
    Ok(())
}

pub(crate) fn extract_typed_with_context(
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
    rules: &Arc<LanguageContext>,
) -> Result<()> {
    extract_file_impl(
        path,
        source,
        module,
        &mut typed.facts,
        Some(&mut typed.flows),
        rules,
    )?;
    javascript::finish_typed(&mut typed.facts, &mut typed.flows);
    typed.project_named_selectors();
    Ok(())
}

#[cfg(test)]
#[test]
fn django_url_tag_emits_named_route_reference() {
    let mut facts = crate::core::models::Facts::default();
    extract_file_impl(
        "templates/products.html",
        "{% url 'product-list' %}",
        "templates.products",
        &mut facts,
        None,
        &dependency_registry::DependencyRegistry::bundled_framework_rules()
            .language_context_for_path("templates/products.html"),
    )
    .unwrap();

    assert!(facts.references.iter().any(|reference| {
        reference.kind == "references_route"
            && reference.expression == "django.route_name.product-list"
    }));
}
