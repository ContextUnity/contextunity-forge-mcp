use tree_sitter::{Node, Parser};

/// Visits TOML key/value pairs with their current dotted table path.
///
/// The value remains a syntax node so callers can handle strings, arrays, and
/// inline tables without reparsing the source text.
pub(crate) fn visit_toml_pairs(source: &str, visitor: &mut impl FnMut(&str, Node<'_>)) {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_toml_ng::language())
        .is_err()
    {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };
    if tree.root_node().has_error() {
        return;
    }

    fn key_path(raw: &str) -> String {
        let mut parts = Vec::new();
        let mut start = 0;
        let mut quote = None;
        for (index, character) in raw.char_indices() {
            match (quote, character) {
                (Some(current), value) if current == value => quote = None,
                (None, '\'' | '"') => quote = Some(character),
                (None, '.') => {
                    parts.push(
                        raw[start..index]
                            .trim()
                            .trim_matches(['\'', '"'])
                            .to_owned(),
                    );
                    start = index + character.len_utf8();
                }
                _ => {}
            }
        }
        parts.push(raw[start..].trim().trim_matches(['\'', '"']).to_owned());
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(".")
    }

    fn visit<'tree>(
        node: Node<'tree>,
        source: &str,
        table: &str,
        visitor: &mut impl FnMut(&str, Node<'tree>),
    ) {
        if node.kind() == "pair" {
            let mut cursor = node.walk();
            let children: Vec<_> = node.named_children(&mut cursor).collect();
            if children.len() >= 2 {
                let key = key_path(&source[children[0].byte_range()]);
                if !key.is_empty() {
                    let full_key = if table.is_empty() {
                        key
                    } else {
                        format!("{table}.{key}")
                    };
                    visitor(&full_key, *children.last().unwrap());
                }
            }
            return;
        }

        let current_table = if matches!(node.kind(), "table" | "table_array_element") {
            let mut cursor = node.walk();
            let header = node
                .named_children(&mut cursor)
                .take_while(|child| child.kind() != "pair")
                .find(|child| matches!(child.kind(), "bare_key" | "quoted_key" | "dotted_key"))
                .map(|child| key_path(&source[child.byte_range()]))
                .unwrap_or_default();
            if table.is_empty() || header.is_empty() {
                header
            } else {
                format!("{table}.{header}")
            }
        } else {
            table.to_owned()
        };

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "bare_key" | "quoted_key" | "dotted_key")
                && matches!(node.kind(), "table" | "table_array_element")
            {
                continue;
            }
            visit(child, source, &current_table, visitor);
        }
    }

    visit(tree.root_node(), source, "", visitor);
}

pub(crate) fn toml_string(node: Node<'_>, source: &str) -> Option<String> {
    let raw = source.get(node.byte_range())?.trim();
    if !matches!(
        node.kind(),
        "string"
            | "basic_string"
            | "literal_string"
            | "multiline_basic_string"
            | "multiline_literal_string"
    ) {
        return None;
    }
    let quote = raw.chars().next()?;
    let multiline = raw.starts_with("'''") || raw.starts_with("\"\"\"");
    let delimiter_len = if multiline { 3 } else { 1 };
    if !raw.ends_with(if multiline {
        if quote == '\'' {
            "'''"
        } else {
            "\"\"\""
        }
    } else if quote == '\'' {
        "'"
    } else {
        "\""
    }) {
        return None;
    }
    let end = raw.len().checked_sub(delimiter_len)?;
    let mut body = raw.get(delimiter_len..end)?;
    if multiline {
        body = body
            .strip_prefix("\r\n")
            .or_else(|| body.strip_prefix('\n'))
            .unwrap_or(body);
    }
    if quote == '\'' {
        return Some(body.to_owned());
    }
    let mut output = String::with_capacity(body.len());
    let mut characters = body.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let escape = characters.next()?;
        match escape {
            'b' => output.push('\u{0008}'),
            't' => output.push('\t'),
            'n' => output.push('\n'),
            'f' => output.push('\u{000c}'),
            'r' => output.push('\r'),
            'e' => output.push('\u{001b}'),
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            '\n' if multiline => {
                while matches!(characters.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                    characters.next();
                }
            }
            '\r' if multiline && characters.peek() == Some(&'\n') => {
                characters.next();
                while matches!(characters.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                    characters.next();
                }
            }
            'x' | 'u' | 'U' => {
                let digits = match escape {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let value = (0..digits).try_fold(0u32, |value, _| {
                    characters
                        .next()?
                        .to_digit(16)
                        .map(|digit| (value << 4) | digit)
                })?;
                output.push(char::from_u32(value)?);
            }
            _ => return None,
        }
    }
    Some(output)
}
