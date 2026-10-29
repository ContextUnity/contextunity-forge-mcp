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
    let mut output = Vec::new();
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
        if matches!(bytes[i], b'\'' | b'"' | b'`' | b'[') {
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
    token.is_some_and(|t| t.text.eq_ignore_ascii_case(value))
}

fn name_at<'a>(tokens: &[Token<'a>], mut i: usize) -> Option<(&'a str, usize)> {
    let mut name = tokens.get(i)?.text.trim_matches(['"', '`', '[', ']']);
    if !name
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
    {
        return None;
    }
    i += 1;
    while tokens.get(i).is_some_and(|t| t.text == ".") {
        name = tokens.get(i + 1)?.text.trim_matches(['"', '`', '[', ']']);
        i += 2;
    }
    Some((name, i))
}

fn table_uses(tokens: &[Token<'_>]) -> Vec<(String, &'static str, usize)> {
    let mut uses = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let (kind, mut next) =
            if token.text.eq_ignore_ascii_case("FROM") || token.text.eq_ignore_ascii_case("JOIN") {
                (
                    if i > 0 && keyword(tokens.get(i - 1), "DELETE") {
                        "mutates_table"
                    } else {
                        "queries_table"
                    },
                    i + 1,
                )
            } else if token.text.eq_ignore_ascii_case("INTO")
                || token.text.eq_ignore_ascii_case("UPDATE")
            {
                ("mutates_table", i + 1)
            } else if token.text.eq_ignore_ascii_case("REFERENCES") {
                ("references_table", i + 1)
            } else {
                continue;
            };
        if keyword(tokens.get(next), "ONLY") {
            next += 1;
        }
        if let Some((name, _)) = name_at(tokens, next) {
            uses.push((name.to_owned(), kind, token.line));
        }
    }
    uses
}

/// Extract table names and relation kinds from an SQL statement.
pub fn extract_embedded_sql_tables(literal: &str) -> Vec<(String, &'static str)> {
    let first = literal
        .trim_start()
        .as_bytes()
        .first()
        .map(u8::to_ascii_uppercase);
    if !matches!(
        first,
        Some(b'S' | b'W' | b'I' | b'U' | b'D' | b'C' | b'-' | b'/')
    ) {
        return Vec::new();
    }
    let tokens = tokens(literal);
    if !tokens.first().is_some_and(|token| {
        ["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "CREATE"]
            .iter()
            .any(|word| token.text.eq_ignore_ascii_case(word))
    }) {
        return Vec::new();
    }
    let mut output: Vec<_> = table_uses(&tokens)
        .into_iter()
        .map(|(name, kind, _)| (name, kind))
        .collect();
    for i in 0..tokens.len() {
        if keyword(tokens.get(i), "CREATE") {
            let mut j = i + 1;
            if keyword(tokens.get(j), "TEMP") || keyword(tokens.get(j), "TEMPORARY") {
                j += 1;
            }
            if keyword(tokens.get(j), "TABLE") {
                j += 1;
                if keyword(tokens.get(j), "IF")
                    && keyword(tokens.get(j + 1), "NOT")
                    && keyword(tokens.get(j + 2), "EXISTS")
                {
                    j += 3;
                }
                if let Some((name, _)) = name_at(&tokens, j) {
                    output.push((name.to_owned(), "defines_table"));
                }
            }
        }
    }
    output
}

pub(super) fn extract_literal(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if !matches!(
        ctx.node.kind(),
        "string_literal" | "raw_string_literal" | "string" | "template_string"
    ) {
        return;
    }
    let raw = text(ctx.node, ctx.source);
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
                        name: name.clone(),
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
                name: name.into(),
                qualname: name.into(),
                path: path.into(),
                line,
                end_line: statement.last().map_or(line, |t| t.line),
                is_test: crate::core::models::is_test(path),
                language: "sql".into(),
                generated: false,
                details: serde_json::json!({"columns": columns, "primary_key": primary}),
                native_semantic: None,
            });
            facts.edges.push(Edge {
                src: format!("module:{path}"),
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
                    is_test: crate::core::models::is_test(path),
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
            let owner = format!("module:{path}");
            if keyword(statement.first(), "ALTER") && keyword(statement.get(1), "TABLE") {
                if let Some((name, after)) = name_at(statement, 2) {
                    reference(
                        facts,
                        &owner,
                        name.into(),
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
                                    name: column.into(),
                                    qualname: format!("{name}.{column}"),
                                    path: path.into(),
                                    line,
                                    end_line: line,
                                    is_test: crate::core::models::is_test(path),
                                    language: "sql".into(),
                                    generated: false,
                                    details: serde_json::Value::Null,
                                    native_semantic: None,
                                });
                                facts.edges.push(Edge {
                                    src: owner.clone(),
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
                reference(facts, &owner, name, relation, line);
            }
        }
    }
}
