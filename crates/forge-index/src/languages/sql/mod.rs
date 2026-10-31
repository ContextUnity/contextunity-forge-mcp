//! Dependency-free SQL declaration and table-reference extraction.
use super::*;
use crate::core::models::Edge;

/// SQL scanner profile.
pub struct Sql;
/// The built-in SQL profile.
pub static SQL: Sql = Sql;

impl LanguageProfile for Sql {
    fn id(&self) -> &'static str {
        "sql"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["sql"]
    }
    fn has_grammar(&self) -> bool {
        false
    }
    fn grammar(&self, _: &str) -> tree_sitter::Language {
        unreachable!("SQL does not use a Tree-sitter grammar")
    }
    fn symbol_kind(&self, _: &str) -> Option<&'static str> {
        None
    }
    fn node_prefix(&self, _: &str) -> &'static str {
        "sql"
    }
    fn normalize_import(&self, _: &str, _: &str) -> Option<ImportPath> {
        None
    }
    fn extract_file(&self, path: &str, source: &str, _: &str, facts: &mut Facts) -> Result<()> {
        extract_schema(path, source, facts);
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    line: usize,
}

fn tokens(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut output = Vec::with_capacity((bytes.len() / 8).min(1024));
    let (mut i, mut line) = (0, 1);
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            line += usize::from(bytes[i] == b'\n');
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"--") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                line += usize::from(bytes[i] == b'\n');
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        let start = i;
        let token_line = line;
        if bytes[i] == b'$'
            && source[i + 1..].find('$').is_some_and(|end| {
                source[i + 1..i + 1 + end]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            let end = source[i + 1..].find('$').expect("checked tag") + i + 1;
            let delimiter = &source[i..=end];
            i = end + 1;
            if let Some(close) = source[i..].find(delimiter) {
                let next = i + close + delimiter.len();
                line += source[i..next].bytes().filter(|b| *b == b'\n').count();
                i = next;
            } else {
                i = bytes.len();
            }
        } else if matches!(bytes[i], b'\'' | b'"' | b'`' | b'[') {
            let close = if bytes[i] == b'[' { b']' } else { bytes[i] };
            i += 1;
            while i < bytes.len() {
                line += usize::from(bytes[i] == b'\n');
                if bytes[i] == close {
                    i += 1;
                    if i < bytes.len() && bytes[i] == close {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
        } else if bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$') {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
            {
                i += 1;
            }
        } else {
            i += source[i..].chars().next().map_or(1, char::len_utf8);
        }
        output.push(Token {
            text: &source[start..i],
            line: token_line,
        });
    }
    output
}

fn keyword(token: Option<&Token<'_>>, value: &str) -> bool {
    token.is_some_and(|t| {
        !t.text.starts_with(['\'', '"', '`', '[', '$']) && t.text.eq_ignore_ascii_case(value)
    })
}

fn name_at(tokens: &[Token<'_>], mut i: usize) -> Option<(String, usize)> {
    let component = |text: &str| {
        let unquoted = text.trim_matches(['"', '`', '[', ']']);
        if text.starts_with(['"', '`', '[']) {
            format!("\"{unquoted}\"")
        } else {
            unquoted.to_owned()
        }
    };
    if tokens.get(i)?.text.starts_with(['\'', '$']) {
        return None;
    }
    let mut name = component(tokens.get(i)?.text);
    if !name
        .trim_start_matches('"')
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
    {
        return None;
    }
    i += 1;
    while tokens.get(i).is_some_and(|t| t.text == ".") {
        name.push('.');
        name.push_str(&component(tokens.get(i + 1)?.text));
        i += 2;
    }
    Some((name, i))
}

/// Canonicalize segment identity: quoting controls case, not equality.
pub(crate) fn identifier_keys(name: &str) -> (String, String, bool) {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut chars = name.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                part.push('"');
            }
            '"' => quoted = !quoted,
            '.' if !quoted => {
                parts.push(std::mem::take(&mut part));
            }
            _ if quoted => part.push(ch),
            _ => part.push(ch.to_ascii_lowercase()),
        }
    }
    parts.push(part);
    let encode = |part: &str| format!("{}:{part}", part.len());
    let short = encode(parts.last().expect("one identifier segment"));
    let qualified = parts.len() > 1;
    let full = parts.iter().map(|part| encode(part)).collect();
    (full, short, qualified)
}

fn identifier_key(name: &str) -> String {
    identifier_keys(name).0
}

fn short_name(name: &str) -> &str {
    let mut quoted = false;
    let mut start = 0;
    for (i, ch) in name.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == '.' && !quoted {
            start = i + 1;
        }
    }
    name[start..].trim_matches('"')
}

fn cte_events(
    tokens: &[Token<'_>],
) -> Option<std::collections::HashMap<usize, Vec<(String, bool)>>> {
    let mut matching = vec![None; tokens.len()];
    let mut stack = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        if token.text == "(" {
            stack.push(i);
        }
        if token.text == ")" {
            let open = stack.pop()?;
            matching[open] = Some(i);
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let mut events: std::collections::HashMap<usize, Vec<(String, bool)>> =
        std::collections::HashMap::new();
    for (i, token) in tokens.iter().enumerate() {
        if token.text == "(" {
            stack.push(i);
            continue;
        }
        if token.text == ")" {
            stack.pop();
            continue;
        }
        if !keyword(Some(token), "WITH") || (i > 0 && tokens[i - 1].text != "(") {
            continue;
        }
        let end = stack
            .last()
            .and_then(|open| matching[*open])
            .unwrap_or(tokens.len());
        let recursive = keyword(tokens.get(i + 1), "RECURSIVE");
        let mut pos = i + 1 + usize::from(recursive);
        loop {
            let (name, mut next) = name_at(tokens, pos)?;
            if identifier_keys(&name).2 {
                return None;
            }
            if tokens.get(next).is_some_and(|token| token.text == "(") {
                next = matching[next]? + 1;
            }
            if !keyword(tokens.get(next), "AS") {
                return None;
            }
            next += 1;
            if keyword(tokens.get(next), "NOT") {
                next += 1;
            }
            if keyword(tokens.get(next), "MATERIALIZED") {
                next += 1;
            }
            if !tokens.get(next).is_some_and(|token| token.text == "(") {
                return None;
            }
            let close = matching[next]?;
            if close >= end {
                return None;
            }
            let key = identifier_key(&name);
            let visible = if recursive { next + 1 } else { close + 1 };
            events.entry(visible).or_default().push((key.clone(), true));
            events.entry(end).or_default().push((key, false));
            pos = close + 1;
            if !tokens.get(pos).is_some_and(|token| token.text == ",") {
                break;
            }
            pos += 1;
        }
    }
    Some(events)
}

fn table_uses(tokens: &[Token<'_>]) -> Vec<(String, &'static str, usize)> {
    let mut uses = Vec::new();
    let has_with = tokens.iter().any(|t| keyword(Some(t), "WITH"));
    let (events, mut ctes) = if has_with {
        let Some(events) = cte_events(tokens) else {
            return uses;
        };
        (Some(events), std::collections::HashMap::<String, usize>::new())
    } else {
        (None, std::collections::HashMap::new())
    };
    let mut operation = 0;
    if keyword(tokens.first(), "WITH") {
        let mut nesting = 0usize;
        for (i, token) in tokens.iter().enumerate().skip(1) {
            if token.text == "(" {
                nesting += 1;
                continue;
            }
            if token.text == ")" {
                nesting = nesting.saturating_sub(1);
                continue;
            }
            if nesting == 0
                && ["SELECT", "INSERT", "UPDATE", "DELETE"]
                    .iter()
                    .any(|word| keyword(Some(token), word))
            {
                operation = i;
                break;
            }
        }
    }
    let mut relations = vec![false];
    let mut depth = 0usize;
    if keyword(tokens.get(operation), "UPDATE") {
        let mut start = operation + 1 + usize::from(keyword(tokens.get(operation + 1), "ONLY"));
        let wrapped = tokens.get(start).is_some_and(|token| token.text == "(");
        start += usize::from(wrapped);
        if let Some((name, mut end)) = name_at(tokens, start) {
            if wrapped {
                if !tokens.get(end).is_some_and(|token| token.text == ")") {
                    return uses;
                }
                end += 1;
            }
            if keyword(tokens.get(end), "AS") {
                end += 2;
            } else if !keyword(tokens.get(end), "SET") && name_at(tokens, end).is_some() {
                end += 1;
            }
            if keyword(tokens.get(end), "SET") {
                uses.push((name, "mutates_table", tokens[operation].line));
            }
        }
    }
    for (i, token) in tokens.iter().enumerate() {
        if let Some(events) = &events {
            if let Some(changes) = events.get(&i) {
                for (key, add) in changes {
                    let count = ctes.entry(key.clone()).or_default();
                    if *add {
                        *count += 1;
                    } else {
                        *count = count.saturating_sub(1);
                    }
                }
            }
        }
        if token.text == "(" {
            depth += 1;
            relations.push(false);
            continue;
        }
        if token.text == ")" {
            if depth == 0 {
                return Vec::new();
            }
            depth -= 1;
            relations.pop();
            continue;
        }
        let from = keyword(Some(token), "FROM");
        let join = keyword(Some(token), "JOIN");
        if from {
            relations[depth] = true;
        }
        if [
            "WHERE",
            "GROUP",
            "ORDER",
            "HAVING",
            "LIMIT",
            "UNION",
            "RETURNING",
            "SET",
        ]
        .iter()
        .any(|word| keyword(Some(token), word))
        {
            relations[depth] = false;
        }
        let kind = if from || join || (token.text == "," && relations[depth]) {
            Some(if from && i > 0 && keyword(tokens.get(i - 1), "DELETE") {
                "mutates_table"
            } else {
                "queries_table"
            })
        } else if keyword(Some(token), "INTO")
            && (keyword(tokens.get(operation), "INSERT")
                || keyword(tokens.get(operation), "REPLACE"))
        {
            Some("mutates_table")
        } else if keyword(Some(token), "REFERENCES") {
            Some("references_table")
        } else {
            None
        };
        if let Some(kind) = kind {
            let mut next = i + 1;
            if keyword(tokens.get(next), "ONLY") {
                next += 1;
            }
            if let Some((name, _)) = name_at(tokens, next) {
                if ctes.is_empty() || {
                    let (key, _, qualified) = identifier_keys(&name);
                    qualified || ctes.get(&key).copied().unwrap_or_default() == 0
                } {
                    uses.push((name, kind, token.line));
                }
            }
        }
    }
    if depth > 0 {
        return Vec::new();
    }
    uses
}

/// Extract table names and relation kinds from an SQL statement.
pub fn extract_embedded_sql_tables(literal: &str) -> Vec<(String, &'static str)> {
    if literal.len() < 12 || !has_sql_statement(literal) {
        return Vec::new();
    }
    let tokens = tokens(literal);
    let statements = tokens.split(|token| token.text == ";");
    let mut output: Vec<_> = statements
        .clone()
        .filter(|statement| {
            statement.first().is_some_and(|token| {
                [
                    "SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER",
                ]
                .iter()
                .any(|word| token.text.eq_ignore_ascii_case(word))
            })
        })
        .flat_map(table_uses)
        .map(|(name, kind, _)| (name, kind))
        .collect();
    for statement in statements {
        if keyword(statement.first(), "CREATE") {
            let mut j = 1;
            if keyword(statement.get(j), "TEMP") || keyword(statement.get(j), "TEMPORARY") {
                j += 1;
            }
            if keyword(statement.get(j), "TABLE") {
                j += 1;
                if keyword(statement.get(j), "IF")
                    && keyword(statement.get(j + 1), "NOT")
                    && keyword(statement.get(j + 2), "EXISTS")
                {
                    j += 3;
                }
                if let Some((name, _)) = name_at(statement, j) {
                    output.push((name.to_owned(), "defines_table"));
                }
            }
        }
    }
    output
}

// Match the statement boundaries used by `tokens`, without allocating tokens for
// ordinary prose. Comments and quoted values can contain semicolons.
fn has_sql_statement(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut first = true;
    while i < bytes.len() {
        let was_first = first;
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"--") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == b';' {
            first = true;
            i += 1;
        } else if matches!(bytes[i], b'\'' | b'"' | b'`' | b'[') {
            let close = if bytes[i] == b'[' { b']' } else { bytes[i] };
            i += 1;
            while i < bytes.len() {
                if bytes[i] == close {
                    i += 1;
                    if i < bytes.len() && bytes[i] == close {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            first = false;
        } else if bytes[i] == b'$' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'$' {
                i += 1;
                let delimiter = &bytes[start..i];
                while i < bytes.len() && !bytes[i..].starts_with(delimiter) {
                    i += 1;
                }
                i = (i + delimiter.len()).min(bytes.len());
            } else {
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
                {
                    i += 1;
                }
            }
            first = false;
        } else if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
            {
                i += 1;
            }
            if first
                && [
                    b"SELECT".as_slice(),
                    b"WITH",
                    b"INSERT",
                    b"UPDATE",
                    b"DELETE",
                    b"CREATE",
                    b"ALTER",
                ]
                .iter()
                .any(|word| bytes[start..i].eq_ignore_ascii_case(word))
            {
                return true;
            }
            first = false;
        } else {
            first = false;
            i += 1;
        }
        if was_first && !first && !bytes[i..].contains(&b';') {
            return false;
        }
    }
    false
}

pub(super) fn extract_literal(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if !matches!(
        ctx.node.kind(),
        "string_literal" | "raw_string_literal" | "string" | "template_string"
    ) {
        return;
    }
    let raw = text(ctx.node, ctx.source);
    if raw.len() < 14 {
        return;
    }
    let Some(start) = raw.find(['\'', '"', '`']) else {
        return;
    };
    let quote = raw.as_bytes()[start] as char;
    let Some(end) = raw.rfind(quote).filter(|end| *end > start) else {
        return;
    };
    let literal = &raw[start + 1..end];
    let literal = if literal.len() >= 4
        && literal.as_bytes()[..2] == [quote as u8; 2]
        && literal.as_bytes()[literal.len() - 2..] == [quote as u8; 2]
    {
        &literal[2..literal.len() - 2]
    } else {
        literal
    };
    if raw[..start].contains('f') || (quote == '`' && literal.contains("${")) {
        return;
    }
    for (name, kind) in extract_embedded_sql_tables(literal) {
        if kind == "defines_table" {
            if let Some(owner) = facts.nodes.iter().find(|node| node.id == ctx.owner) {
                let path = owner.path.clone();
                let id = format!("{path}:table:{name}");
                if !facts.nodes.iter().any(|node| node.id == id) {
                    facts.nodes.push(Node {
                        id: id.clone(),
                        kind: "table".into(),
                        name: short_name(&name).to_owned(),
                        qualname: name.clone(),
                        path: path.clone(),
                        line: ctx.line(),
                        end_line: ctx.line(),
                        is_test: crate::core::models::is_test(&path),
                        language: "sql".into(),
                        generated: true,
                        details: serde_json::Value::Null,
                        native_semantic: None,
                    });
                    facts.edges.push(Edge {
                        src: ctx.owner.into(),
                        dst: id,
                        kind: "contains".into(),
                        path,
                        line: ctx.line(),
                        evidence: "embedded SQL".into(),
                        confidence: "exact".into(),
                    });
                }
            }
        }
        reference(facts, ctx.owner, name, kind, ctx.line());
    }
}

/// Records a configured Rust macro's literal query and result-type candidate.
pub(super) fn extract_macro_type(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if ctx.node.kind() != "macro_invocation" {
        return;
    }
    let Some(name) = ctx.node.child_by_field_name("macro") else {
        return;
    };
    let macro_path = text(name, ctx.source);
    let Some(tree) = ctx
        .node
        .named_children(&mut ctx.node.walk())
        .find(|node| node.kind() == "token_tree")
    else {
        return;
    };
    let mut cursor = tree.walk();
    let mut children = tree.named_children(&mut cursor);
    let Some(ty) = children
        .next()
        .filter(|node| matches!(node.kind(), "identifier" | "type_identifier"))
    else {
        return;
    };
    let Some(literal) = children
        .next()
        .filter(|node| matches!(node.kind(), "string_literal" | "raw_string_literal"))
    else {
        return;
    };
    let raw = text(literal, ctx.source);
    let Some(start) = raw.find('"') else { return };
    let Some(end) = raw.rfind('"').filter(|end| *end > start) else {
        return;
    };
    for (name, kind) in extract_embedded_sql_tables(&raw[start + 1..end]) {
        if kind == "queries_table" {
            facts.references.push(Reference {
                source: ctx.owner.into(),
                expression: name,
                kind: "sql_type_queries_table".into(),
                line: ctx.line(),
                column: ctx.node.start_position().column,
                alias: Some(text(ty, ctx.source).into()),
                module: Some(macro_path.into()),
                dynamic: false,
                receiver_hint: None,
            });
        }
    }
}

fn reference(facts: &mut Facts, owner: &str, name: String, kind: &str, line: usize) {
    facts.references.push(Reference {
        source: owner.into(),
        expression: name,
        kind: kind.into(),
        line,
        column: 0,
        alias: None,
        module: None,
        dynamic: false,
        receiver_hint: None,
    });
}

fn extract_schema(path: &str, source: &str, facts: &mut Facts) {
    let is_test = crate::core::models::is_test(path);
    let module_owner = format!("module:{path}");
    let tokens = tokens(source);
    for statement in tokens.split(|t| t.text == ";").filter(|s| !s.is_empty()) {
        let mut pos = 0;
        let is_create = keyword(statement.get(pos), "CREATE");
        if is_create {
            pos += 1;
            if keyword(statement.get(pos), "TEMP") || keyword(statement.get(pos), "TEMPORARY") {
                pos += 1;
            }
            let kind = if keyword(statement.get(pos), "TABLE") {
                "table"
            } else if keyword(statement.get(pos), "VIEW") {
                "view"
            } else {
                continue;
            };
            pos += 1;
            if keyword(statement.get(pos), "IF")
                && keyword(statement.get(pos + 1), "NOT")
                && keyword(statement.get(pos + 2), "EXISTS")
            {
                pos += 3;
            }
            let Some((name, after)) = name_at(statement, pos) else {
                continue;
            };
            let id = format!("{path}:{kind}:{name}");
            let line = statement[pos].line;
            let mut columns = Vec::new();
            let mut column_lines = Vec::new();
            let mut primary = Vec::new();
            if kind == "table" && statement.get(after).is_some_and(|t| t.text == "(") {
                let mut depth = 0usize;
                let mut start = after + 1;
                for end in after + 1..statement.len() {
                    let value = statement[end].text;
                    if value == "(" {
                        depth += 1;
                    }
                    if value == ")" && depth > 0 {
                        depth -= 1;
                        continue;
                    }
                    if (value == "," && depth == 0) || (value == ")" && depth == 0) {
                        let chunk = &statement[start..end];
                        if keyword(chunk.first(), "PRIMARY") && keyword(chunk.get(1), "KEY") {
                            primary.extend(chunk.iter().skip(2).filter_map(|token| {
                                let name = token.text.trim_matches(['"', '`', '[', ']']);
                                name.as_bytes()
                                    .first()
                                    .filter(|b| b.is_ascii_alphabetic() || **b == b'_')
                                    .map(|_| name.to_owned())
                            }));
                        }
                        if let Some((column, _)) = name_at(chunk, 0) {
                            if !["PRIMARY", "FOREIGN", "CONSTRAINT", "UNIQUE", "CHECK"]
                                .iter()
                                .any(|word| column.eq_ignore_ascii_case(word))
                            {
                                columns.push(column.to_owned());
                                column_lines.push(chunk[0].line);
                                if chunk.windows(2).any(|pair| {
                                    keyword(pair.first(), "PRIMARY") && keyword(pair.get(1), "KEY")
                                }) {
                                    primary.push(column.to_owned());
                                }
                            }
                        }
                        start = end + 1;
                        if value == ")" {
                            break;
                        }
                    }
                }
            }
            facts.nodes.push(Node {
                id: id.clone(),
                kind: kind.into(),
                name: short_name(&name).into(),
                qualname: name.clone(),
                path: path.into(),
                line,
                end_line: statement.last().map_or(line, |t| t.line),
                is_test,
                language: "sql".into(),
                generated: false,
                details: serde_json::json!({"columns": columns, "primary_key": primary}),
                native_semantic: None,
            });
            facts.edges.push(Edge {
                src: module_owner.clone(),
                dst: id.clone(),
                kind: "contains".into(),
                path: path.into(),
                line,
                evidence: kind.into(),
                confidence: "exact".into(),
            });
            for (column, column_line) in columns.iter().zip(column_lines) {
                let column_id = format!("{path}:column:{name}.{column}");
                facts.nodes.push(Node {
                    id: column_id.clone(),
                    kind: "column".into(),
                    name: column.clone(),
                    qualname: format!("{name}.{column}"),
                    path: path.into(),
                    line: column_line,
                    end_line: column_line,
                    is_test,
                    language: "sql".into(),
                    generated: false,
                    details: serde_json::Value::Null,
                    native_semantic: None,
                });
                facts.edges.push(Edge {
                    src: id.clone(),
                    dst: column_id,
                    kind: "contains".into(),
                    path: path.into(),
                    line: column_line,
                    evidence: "column".into(),
                    confidence: "exact".into(),
                });
            }
            for (name, relation, line) in table_uses(statement) {
                reference(facts, &id, name, relation, line);
            }
        } else {
            if keyword(statement.first(), "ALTER") && keyword(statement.get(1), "TABLE") {
                if let Some((name, after)) = name_at(statement, 2) {
                    reference(
                        facts,
                        &module_owner,
                        name.clone(),
                        "mutates_table",
                        statement[0].line,
                    );
                    if keyword(statement.get(after), "ADD") {
                        let column_index =
                            after + 1 + usize::from(keyword(statement.get(after + 1), "COLUMN"));
                        if let Some((column, _)) = name_at(statement, column_index) {
                            if !["CONSTRAINT", "PRIMARY", "FOREIGN", "UNIQUE", "CHECK"]
                                .iter()
                                .any(|word| column.eq_ignore_ascii_case(word))
                            {
                                let column_id = format!("{path}:column:{name}.{column}");
                                let line = statement[column_index].line;
                                facts.nodes.push(Node {
                                    id: column_id.clone(),
                                    kind: "column".into(),
                                    name: column.clone(),
                                    qualname: format!("{name}.{column}"),
                                    path: path.into(),
                                    line,
                                    end_line: line,
                                    is_test,
                                    language: "sql".into(),
                                    generated: false,
                                    details: serde_json::Value::Null,
                                    native_semantic: None,
                                });
                                facts.edges.push(Edge {
                                    src: module_owner.clone(),
                                    dst: column_id,
                                    kind: "contains".into(),
                                    path: path.into(),
                                    line,
                                    evidence: "alter column".into(),
                                    confidence: "exact".into(),
                                });
                            }
                        }
                    }
                }
            }
            for (name, relation, line) in table_uses(statement) {
                reference(facts, &module_owner, name, relation, line);
            }
        }
    }
}
