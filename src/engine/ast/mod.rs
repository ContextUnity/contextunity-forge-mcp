pub mod go;
pub mod proto;
pub mod python;
pub mod rust;
pub mod typescript;
use crate::core::models::*;
use anyhow::{bail, Context, Result};
use serde_json::json;
use tree_sitter::{Node as Syntax, Parser, Tree};
pub fn parser(language: &str, path: &str) -> Result<Parser> {
    let grammar = match language {
        "python" => python::language(),
        "typescript" | "javascript" | "vue" => typescript::language(path, language),
        "rust" => rust::language(),
        "go" => go::language(),
        "proto" => proto::language(),
        _ => bail!("unsupported AST language: {language}"),
    };
    let mut p = Parser::new();
    p.set_language(&grammar)?;
    Ok(p)
}
pub fn text<'a>(node: Syntax<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
fn field<'a>(node: Syntax<'_>, source: &'a str, name: &str) -> Option<&'a str> {
    node.child_by_field_name(name).map(|n| text(n, source))
}
fn symbol_kind(language: &str, kind: &str) -> Option<&'static str> {
    match language {
        "python" => python::kind(kind),
        "rust" => rust::kind(kind),
        "go" => go::kind(kind),
        "proto" => proto::kind(kind),
        _ => typescript::kind(kind),
    }
}
fn qualified(module: &str, scopes: &[String], name: &str) -> String {
    std::iter::once(module)
        .chain(scopes.iter().map(String::as_str))
        .chain(std::iter::once(name))
        .collect::<Vec<_>>()
        .join(".")
}
fn doc_comment(node: Syntax<'_>, source: &str, language: &str) -> String {
    if language == "python" {
        if let Some(body) = node.child_by_field_name("body") {
            if let Some(first) = body.named_child(0) {
                if first.kind() == "expression_statement"
                    && first.named_child(0).is_some_and(|n| n.kind() == "string")
                {
                    return text(first, source).to_owned();
                }
            }
        }
    }
    let mut anchor = node;
    while let Some(parent) = anchor.parent() {
        if matches!(
            parent.kind(),
            "export_statement"
                | "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
                | "decorated_definition"
        ) {
            anchor = parent;
        } else {
            break;
        }
    }
    let mut comments = Vec::new();
    let mut previous = anchor.prev_named_sibling();
    while let Some(n) = previous {
        if !n.kind().contains("comment") {
            break;
        }
        comments.push(text(n, source));
        previous = n.prev_named_sibling();
    }
    comments.reverse();
    comments.join("\n")
}
fn scope_bindings(node: Syntax<'_>, source: &str) -> Vec<String> {
    fn names(node: Syntax<'_>, source: &str, out: &mut std::collections::BTreeSet<String>) {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                out.insert(text(node, source).into());
            }
            "default_parameter"
            | "typed_default_parameter"
            | "required_parameter"
            | "optional_parameter"
            | "parameter"
            | "parameter_declaration"
            | "variadic_parameter_declaration" => {
                for field in ["name", "pattern"] {
                    let mut c = node.walk();
                    for child in node.children_by_field_name(field, &mut c) {
                        names(child, source, out);
                    }
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(n) = node.child_by_field_name("left") {
                    names(n, source, out);
                }
            }
            "pair_pattern" => {
                if let Some(n) = node.child_by_field_name("value") {
                    names(n, source, out);
                }
            }
            "typed_parameter" => {
                if let Some(n) = node.named_child(0) {
                    if Some(n) != node.child_by_field_name("type") {
                        names(n, source, out);
                    }
                }
            }
            "parameters"
            | "formal_parameters"
            | "parameter_list"
            | "tuple_pattern"
            | "list_pattern"
            | "pattern_list"
            | "expression_list"
            | "array_pattern"
            | "object_pattern"
            | "rest_pattern"
            | "list_splat_pattern"
            | "dictionary_splat_pattern"
            | "tuple_struct_pattern"
            | "slice_pattern"
            | "reference_pattern"
            | "mut_pattern" => {
                let mut c = node.walk();
                for child in node.named_children(&mut c) {
                    names(child, source, out);
                }
            }
            _ => {}
        }
    }
    let mut out = std::collections::BTreeSet::new();
    for field in ["parameters", "parameter"] {
        if let Some(parameters) = node.child_by_field_name(field) {
            names(parameters, source, &mut out);
        }
    }
    let mut stack = node
        .child_by_field_name("body")
        .into_iter()
        .collect::<Vec<_>>();
    while let Some(n) = stack.pop() {
        if matches!(
            n.kind(),
            "function_definition"
                | "class_definition"
                | "function_declaration"
                | "function_item"
                | "arrow_function"
                | "function_expression"
                | "method_definition"
        ) {
            continue;
        }
        let lhs = match n.kind() {
            "assignment"
            | "augmented_assignment"
            | "for_statement"
            | "for_in_statement"
            | "short_var_declaration" => n.child_by_field_name("left"),
            "variable_declarator" | "let_declaration" => n
                .child_by_field_name("name")
                .or_else(|| n.child_by_field_name("pattern")),
            _ => None,
        };
        let callable = n
            .child_by_field_name("value")
            .or_else(|| n.child_by_field_name("right"))
            .is_some_and(|n| matches!(n.kind(), "arrow_function" | "function_expression"));
        if !callable {
            if let Some(lhs) = lhs {
                names(lhs, source, &mut out);
            }
        }
        let mut c = n.walk();
        stack.extend(n.named_children(&mut c));
    }
    out.into_iter().collect()
}
fn declaration_signature(node: Syntax<'_>, source: &str) -> String {
    let mut c = node.walk();
    let body_node = node
        .child_by_field_name("body")
        .or_else(|| {
            node.named_children(&mut c)
                .find(|ch| matches!(ch.kind(), "message_body" | "enum_body"))
        });
    let end = body_node.map_or(node.end_byte(), |b| b.start_byte());
    source[node.start_byte()..end]
        .trim()
        .trim_end_matches(';')
        .trim()
        .chars()
        .take(512)
        .collect()
}
fn bounded_expression(value: &str) -> String {
    if value.len() <= 512 {
        return value.into();
    }
    format!(
        "{}… [sha256:{}]",
        value.chars().take(256).collect::<String>(),
        crate::core::commitments::hash(value.as_bytes())
    )
}
fn is_default_export(mut node: Syntax<'_>, source: &str, name: &str) -> bool {
    let original = node;
    while let Some(parent) = node.parent() {
        if parent.kind() == "export_statement"
            && text(parent, source)
                .trim_start()
                .starts_with("export default")
        {
            return true;
        }
        if !matches!(
            parent.kind(),
            "variable_declarator" | "lexical_declaration" | "variable_declaration"
        ) {
            break;
        }
        node = parent;
    }
    let mut anchor = original;
    while let Some(parent) = anchor.parent() {
        if matches!(parent.kind(), "program" | "module") {
            let mut c = parent.walk();
            return parent.named_children(&mut c).any(|n| {
                if n.kind() != "export_statement" || n.child_by_field_name("source").is_some() {
                    return false;
                }
                if text(n, source).trim_start().starts_with("export default")
                    && n.child_by_field_name("value")
                        .is_some_and(|v| v.kind() == "identifier" && text(v, source) == name)
                {
                    return true;
                }
                let mut c = n.walk();
                for clause in n
                    .named_children(&mut c)
                    .filter(|n| n.kind() == "export_clause")
                {
                    let mut cursor = clause.walk();
                    for specifier in clause.named_children(&mut cursor) {
                        if field(specifier, source, "name") == Some(name)
                            && field(specifier, source, "alias") == Some("default")
                        {
                            return true;
                        }
                    }
                }
                false
            });
        }
        if !matches!(
            parent.kind(),
            "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
                | "export_statement"
        ) {
            break;
        }
        anchor = parent;
    }
    false
}
fn record_import(node: Syntax<'_>, source: &str, owner: &str, facts: &mut Facts) {
    let statement = text(node, source);
    let line = node.start_position().row + 1;
    let mut add = |expression: String, alias: Option<String>, module: Option<String>| {
        facts.references.push(Reference {
            source: owner.into(),
            dynamic: false,
            expression,
            kind: "imports".into(),
            line,
            alias,
            module,
        })
    };
    match node.kind() {
        "import_from_statement" => {
            let module = field(node, source, "module_name").unwrap_or("").to_owned();
            let mut c = node.walk();
            for child in node.children_by_field_name("name", &mut c) {
                let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                add(
                    name.into(),
                    field(child, source, "alias")
                        .map(str::to_owned)
                        .or_else(|| Some(name.into())),
                    Some(module.clone()),
                );
            }
            if statement.contains('*') {
                add("*".into(), None, Some(module));
            }
        }
        "import_statement"
            if statement.starts_with("import ")
                && !statement.contains("from ")
                && !statement.contains('"')
                && !statement.contains('\'') =>
        {
            let mut c = node.walk();
            for child in node.children_by_field_name("name", &mut c) {
                let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                add(
                    name.into(),
                    field(child, source, "alias")
                        .map(str::to_owned)
                        .or_else(|| Some(name.split('.').next().unwrap_or(name).into())),
                    Some(name.into()),
                );
            }
        }
        "import_statement" => {
            let module =
                field(node, source, "source").map(|s| s.trim_matches(['\'', '"']).to_owned());
            fn ids(node: Syntax<'_>, source: &str, values: &mut Vec<(String, Option<String>)>) {
                match node.kind() {
                    "import_specifier" => {
                        let name =
                            field(node, source, "name").unwrap_or_else(|| text(node, source));
                        values.push((
                            name.into(),
                            field(node, source, "alias")
                                .map(str::to_owned)
                                .or_else(|| Some(name.into())),
                        ));
                    }
                    "namespace_import" => {
                        if let Some(n) = node.named_child(0) {
                            values.push(("*".into(), Some(text(n, source).into())));
                        }
                    }
                    "identifier" => {
                        values.push(("default".into(), Some(text(node, source).into())))
                    }
                    _ => {
                        let mut c = node.walk();
                        for n in node.named_children(&mut c) {
                            ids(n, source, values);
                        }
                    }
                }
            }
            let mut names = Vec::new();
            let mut c = node.walk();
            for n in node.named_children(&mut c) {
                if n.kind() == "import_clause" {
                    ids(n, source, &mut names);
                }
            }
            if names.is_empty() {
                add("*".into(), None, module);
            } else {
                for (name, alias) in names {
                    add(name, alias, module.clone());
                }
            }
        }
        "use_declaration" => {
            let value = field(node, source, "argument")
                .unwrap_or(statement.trim_start_matches("use ").trim_end_matches(';'));
            fn expand(value: &str, prefix: &str, out: &mut Vec<(String, String)>) {
                if let Some((base, rest)) = value.split_once('{') {
                    let root = format!("{prefix}{}", base.trim());
                    for item in rest.trim_end_matches('}').split(',') {
                        if !item.trim().is_empty() {
                            expand(item.trim(), &root, out);
                        }
                    }
                } else {
                    let (name, alias) = value
                        .split_once(" as ")
                        .unwrap_or((value, value.rsplit("::").next().unwrap_or(value)));
                    out.push((format!("{prefix}{}", name.trim()), alias.trim().into()));
                }
            }
            let mut names = Vec::new();
            expand(value, "", &mut names);
            for (name, alias) in names {
                add(name.clone(), Some(alias), Some(name));
            }
        }
        "import_spec" => {
            if let Some(path) = field(node, source, "path") {
                let path = path.trim_matches(['"', '`']);
                add(
                    path.into(),
                    Some(
                        field(node, source, "name")
                            .unwrap_or(path.rsplit('/').next().unwrap_or(path))
                            .into(),
                    ),
                    Some(path.into()),
                );
            }
        }
        "import" => {
            let mut p = field(node, source, "path");
            if p.is_none() {
                let mut c = node.walk();
                p = node
                    .named_children(&mut c)
                    .find(|n| n.kind() == "string")
                    .map(|n| text(n, source));
            }
            if let Some(path) = p {
                let path = path.trim_matches(['"', '\'', '`']);
                add(
                    path.into(),
                    Some(path.rsplit('/').next().unwrap_or(path).into()),
                    Some(path.into()),
                );
            }
        }
        _ => {}
    }
}

fn visit(
    node: Syntax<'_>,
    source: &str,
    path: &str,
    language: &str,
    module: &str,
    scopes: &mut Vec<String>,
    owner: &str,
    facts: &mut Facts,
    offset: usize,
) {
    if node.is_error() || node.is_missing() {
        facts.errors.push(Diagnostic {
            path: path.into(),
            line: node.start_position().row + offset + 1,
            message: format!(
                "syntax {} at column {}",
                node.kind(),
                node.start_position().column + 1
            ),
        });
    }
    let mut child_owner = owner.to_owned();
    let mut pushed = false;
    if let Some(kind) = symbol_kind(language, node.kind()) {
        let inferred = node.parent().and_then(|p| {
            if matches!(p.kind(), "variable_declarator" | "pair" | "assignment") {
                field(p, source, "name")
                    .or_else(|| field(p, source, "left"))
                    .or_else(|| field(p, source, "key"))
            } else {
                None
            }
        });
        let mut proto_cursor = node.walk();
        let proto_name = if language == "proto" {
            node.named_children(&mut proto_cursor)
                .find(|ch| {
                    matches!(
                        ch.kind(),
                        "message_name" | "service_name" | "rpc_name" | "enum_name"
                    )
                })
                .map(|ch| text(ch, source))
        } else {
            None
        };
        let name = proto_name
            .or_else(|| field(node, source, "name"))
            .or_else(|| field(node, source, "type"))
            .or(inferred)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "anonymous@{}:{}",
                    node.start_position().row + offset + 1,
                    node.start_position().column + 1
                )
            });
        let prefix = match (language, kind) {
            ("python", "class") => "class",
            ("python", _) => "py",
            ("rust", _) => "rs",
            ("go", _) => "go",
            ("proto", "class") => "message",
            ("proto", "service") => "service",
            ("proto", "function") => "rpc",
            ("proto", "enum") => "enum",
            _ => "ts",
        };
        let line = node.start_position().row + offset + 1;
        let mut id = format!("{prefix}:{path}:{line}:{name}");
        if facts.nodes.iter().any(|n| n.id == id) {
            id.push_str(&format!(":{}", node.start_position().column));
        }
        if language == "proto" && node.kind() == "rpc" {
            let mut c = node.walk();
            for ch in node.named_children(&mut c) {
                if ch.kind() == "message_or_enum_type" {
                    let type_name = text(ch, source).trim();
                    if !type_name.is_empty() {
                        facts.references.push(Reference {
                            source: id.clone(),
                            dynamic: false,
                            expression: type_name.to_owned(),
                            kind: "references".into(),
                            line,
                            alias: None,
                            module: type_name.rsplit_once('.').map(|(m, _)| m.to_owned()),
                        });
                    }
                }
            }
        }
        let decorators = node
            .parent()
            .filter(|p| p.kind() == "decorated_definition")
            .map(|p| {
                let mut c = p.walk();
                p.named_children(&mut c)
                    .filter(|n| n.kind() == "decorator")
                    .map(|n| text(n, source).to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let details = json!({"bindings":scope_bindings(node,source),"default_export":is_default_export(node,source,&name),"doc":doc_comment(node,source,language),"decorators":decorators,"bases":field(node,source,"superclasses"),"receiver":field(node,source,"receiver"),"signature":declaration_signature(node,source),"async":text(node,source).trim_start().starts_with("async ")});
        facts.nodes.push(Node {
            id: id.clone(),
            kind: kind.into(),
            name: name.clone(),
            qualname: qualified(module, scopes, &name),
            path: path.into(),
            line,
            end_line: node.end_position().row + offset + 1,
            is_test: is_test(path),
            language: language.into(),
            generated: false,
            details,
        });
        facts.edges.push(Edge {
            src: owner.into(),
            dst: id.clone(),
            kind: "contains".into(),
            path: path.into(),
            line,
            evidence: node.kind().into(),
            confidence: "exact".into(),
        });
        child_owner = id;
        scopes.push(name);
        pushed = true;
    }
    if matches!(
        node.kind(),
        "import_statement" | "import_from_statement" | "use_declaration" | "import_spec" | "import"
    ) {
        let before = facts.references.len();
        record_import(node, source, &format!("module:{path}"), facts);
        for r in &mut facts.references[before..] {
            r.line += offset;
        }
    }
    if matches!(node.kind(), "call" | "call_expression" | "macro_invocation") {
        if let Some(callee) =
            field(node, source, "function").or_else(|| field(node, source, "macro"))
        {
            let dynamic = callee == "import" || callee == "require";
            let import_path = if dynamic {
                node.child_by_field_name("arguments")
                    .and_then(|n| n.named_child(0))
                    .filter(|n| n.kind() == "string")
                    .map(|n| text(n, source).trim_matches(['\'', '"']).to_owned())
            } else {
                None
            };
            facts.references.push(Reference {
                source: child_owner.clone(),
                dynamic: !callee
                    .chars()
                    .all(|c| c.is_alphanumeric() || "_.:!".contains(c)),
                expression: import_path
                    .clone()
                    .unwrap_or_else(|| bounded_expression(callee)),
                kind: if dynamic { "imports" } else { "calls" }.into(),
                line: node.start_position().row + offset + 1,
                alias: None,
                module: import_path,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(
            child,
            source,
            path,
            language,
            module,
            scopes,
            &child_owner,
            facts,
            offset,
        );
    }
    if pushed {
        scopes.pop();
    }
}
pub fn extract(path: &str, language: &str, source: &str) -> Result<Facts> {
    let module = module_name(path);
    let mut facts = Facts::default();
    facts.nodes.push(Node {
        id: format!("module:{path}"),
        kind: "module".into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        qualname: module.clone(),
        path: path.into(),
        line: 1,
        end_line: source.lines().count().max(1),
        is_test: is_test(path),
        language: language.into(),
        generated: false,
        details: json!({}),
    });

    if language == "vue" {
        let mut rest = source;
        let mut base = 0;
        while let Some(start) = rest.find("<script") {
            let opening = start
                + rest[start..]
                    .find('>')
                    .context("unterminated Vue script tag")?
                + 1;
            let end = opening
                + rest[opening..]
                    .find("</script>")
                    .context("unterminated Vue script block")?;
            let script = &rest[opening..end];
            let lang = if rest[start..opening].contains("lang=\"ts\"")
                || rest[start..opening].contains("lang='ts'")
            {
                "typescript"
            } else {
                "javascript"
            };
            let tree = parser(lang, path)?
                .parse(script, None)
                .context("Tree-sitter parse cancelled")?;
            let offset = source[..base + opening]
                .bytes()
                .filter(|b| *b == b'\n')
                .count();
            visit(
                tree.root_node(),
                script,
                path,
                lang,
                &module,
                &mut Vec::new(),
                &format!("module:{path}"),
                &mut facts,
                offset,
            );
            base += end + 9;
            rest = &source[base..];
        }
    } else {
        let tree = parser(language, path)?
            .parse(source, None)
            .context("Tree-sitter parse cancelled")?;
        visit(
            tree.root_node(),
            source,
            path,
            language,
            &module,
            &mut Vec::new(),
            &format!("module:{path}"),
            &mut facts,
            0,
        );
    }
    Ok(facts)
}

fn structural_match(
    pattern: Syntax<'_>,
    target: Syntax<'_>,
    ps: &str,
    source: &str,
    captures: &mut serde_json::Map<String, serde_json::Value>,
) -> bool {
    let token = text(pattern, ps);
    if token.starts_with("__FORGE_META_") && pattern.named_child_count() == 0 {
        let name = token.trim_start_matches("__FORGE_META_");
        let value = json!(text(target, source));
        return match captures.get(name) {
            Some(old) => *old == value,
            None => {
                captures.insert(name.into(), value);
                true
            }
        };
    }
    if pattern.kind() != target.kind() {
        return false;
    }
    if pattern.child_count() == 0 {
        return token == text(target, source);
    }
    let mut pc = pattern.walk();
    let p: Vec<_> = pattern
        .children(&mut pc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    let mut tc = target.walk();
    let t: Vec<_> = target
        .children(&mut tc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    fn sequence(
        p: &[Syntax<'_>],
        t: &[Syntax<'_>],
        ps: &str,
        ts: &str,
        caps: &mut serde_json::Map<String, serde_json::Value>,
    ) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        let token = text(p[0], ps);
        if token.starts_with("__FORGE_MANY_") {
            for count in 0..=t.len() {
                let mut branch = caps.clone();
                let name = token.trim_start_matches("__FORGE_MANY_");
                let value = if count == 0 {
                    ""
                } else {
                    &ts[t[0].start_byte()..t[count - 1].end_byte()]
                };
                if branch.get(name).is_some_and(|v| v != value) {
                    continue;
                }
                branch.insert(name.into(), json!(value));
                if sequence(&p[1..], &t[count..], ps, ts, &mut branch) {
                    *caps = branch;
                    return true;
                }
            }
            return false;
        }
        if t.is_empty() || !structural_match(p[0], t[0], ps, ts, caps) {
            return false;
        }
        sequence(&p[1..], &t[1..], ps, ts, caps)
    }
    sequence(&p, &t, ps, source, captures)
}
pub fn search(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    let mut normalized = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut count = 1;
            while chars.peek() == Some(&'$') {
                chars.next();
                count += 1;
            }
            normalized.push_str(if count > 1 {
                "__FORGE_MANY_"
            } else {
                "__FORGE_META_"
            });
        } else {
            normalized.push(c);
        }
    }
    let partial_body = language == "python" && normalized.trim_end().ends_with(':');
    if partial_body {
        normalized.push_str("\n    __FORGE_META_BODY\n");
    }
    let mut parser = parser(language, path)?;
    let pt: Tree = parser.parse(&normalized, None).context("invalid pattern")?;
    if pt.root_node().has_error() {
        bail!("pattern is not valid {language} syntax");
    }
    let mut pn = pt.root_node();
    while pn.named_child_count() == 1
        && matches!(
            pn.kind(),
            "module" | "program" | "source_file" | "expression_statement"
        )
    {
        pn = pn.named_child(0).context("empty pattern")?;
    }
    let tree = parser.parse(source, None).context("parse cancelled")?;
    let mut stack = vec![tree.root_node()];
    let mut matches = Vec::new();
    while let Some(n) = stack.pop() {
        let mut captures = serde_json::Map::new();
        let matched = if partial_body && n.kind() == pn.kind() {
            let mut pc = pn.walk();
            let mut nc = n.walk();
            let a: Vec<_> = pn
                .children(&mut pc)
                .filter(|x| x.kind() != "block")
                .collect();
            let b: Vec<_> = n
                .children(&mut nc)
                .filter(|x| x.kind() != "block")
                .collect();
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| structural_match(*a, b, &normalized, source, &mut captures))
        } else {
            structural_match(pn, n, &normalized, source, &mut captures)
        };
        if matched {
            matches.push(json!({"path":path,"line":n.start_position().row+1,"end_line":n.end_position().row+1,"text":text(n,source),"captures":captures}));
            if matches.len() >= limit {
                break;
            }
        }
        let mut c = n.walk();
        let children: Vec<_> = n.named_children(&mut c).collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(matches)
}
