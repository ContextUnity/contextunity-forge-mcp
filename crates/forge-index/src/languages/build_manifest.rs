const CONFIGURATIONS: &[&str] = &[
    "api",
    "implementation",
    "compileOnly",
    "runtimeOnly",
    "annotationProcessor",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "testAnnotationProcessor",
    "kapt",
    "ksp",
    "classpath",
];

pub(crate) fn dependencies(filename: &str, content: &str) -> Vec<String> {
    match filename {
        "pom.xml" => maven_groups(content),
        "build.gradle" | "build.gradle.kts" => gradle_groups(content),
        _ => Vec::new(),
    }
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !value.starts_with("${") && !out.iter().any(|old| old == value) {
        out.push(value.to_owned());
    }
}

fn tag_value<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let opening = format!("<{tag}>");
    let closing = format!("</{tag}>");
    let start = block.find(&opening)? + opening.len();
    let end = block[start..].find(&closing)? + start;
    Some(block[start..end].trim())
}

fn maven_groups(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(relative_start) = content[search_from..].find("<dependency") {
        let start = search_from + relative_start;
        let after_name = start + "<dependency".len();
        if content
            .as_bytes()
            .get(after_name)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b'>' | b'/'))
        {
            search_from = after_name;
            continue;
        }
        let Some(relative_end) = content[after_name..].find("</dependency>") else {
            break;
        };
        let end = after_name + relative_end;
        let block = &content[after_name..end];
        if let Some(group) = tag_value(block, "groupId") {
            push_unique(&mut out, group);
        }
        search_from = end + "</dependency>".len();
    }
    out
}

fn gradle_groups(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < content.len() {
        let Some((name, start)) = next_configuration(content, offset) else {
            break;
        };
        offset = start;
        let rest = content[start..].trim_start();
        if let Some(open) = rest.strip_prefix('(') {
            if let Some(end) = balanced_call_end(open) {
                if let Some(coordinate) = first_quoted(open.get(..end).unwrap_or(open)) {
                    push_unique(&mut out, coordinate_group(coordinate).unwrap_or(coordinate));
                }
            }
        } else if rest.starts_with('\'') || rest.starts_with('"') {
            if let Some(coordinate) = first_quoted(rest) {
                push_unique(&mut out, coordinate_group(coordinate).unwrap_or(coordinate));
            }
        } else if rest.starts_with("group:") {
            if let Some(group) = first_quoted(rest) {
                push_unique(&mut out, group);
            }
        }
        let _ = name;
    }
    out
}

fn next_configuration(content: &str, offset: usize) -> Option<(&'static str, usize)> {
    CONFIGURATIONS
        .iter()
        .filter_map(|name| {
            content[offset..]
                .match_indices(name)
                .find_map(|(relative, _)| {
                    let start = offset + relative;
                    let end = start + name.len();
                    let before = content[..start].chars().next_back();
                    let after = content[end..].trim_start().chars().next();
                    let valid_before =
                        before.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
                    let valid_after = matches!(after, Some('(' | '\'' | '"'))
                        || content[end..].trim_start().starts_with("group:");
                    (valid_before && valid_after).then_some((*name, end))
                })
        })
        .min_by_key(|(_, end)| *end)
}

fn balanced_call_end(arguments: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
        if let Some(current) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == current {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_quoted(text: &str) -> Option<&str> {
    let mut chars = text.char_indices();
    let (start, quote) = chars.find(|(_, character)| matches!(character, '\'' | '"'))?;
    let content_start = start + quote.len_utf8();
    let mut escaped = false;
    for (relative, character) in text[content_start..].char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == quote {
            return Some(&text[content_start..content_start + relative]);
        }
    }
    None
}

fn coordinate_group(coordinate: &str) -> Option<&str> {
    let mut parts = coordinate.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    (!group.is_empty() && !artifact.is_empty()).then_some(group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_maven_and_gradle_dependency_group_prefixes() {
        assert_eq!(
            maven_groups(
                "<dependencies><dependency><groupId>org.example</groupId><artifactId>lib</artifactId></dependency></dependencies>"
            ),
            ["org.example"]
        );
        assert_eq!(
            gradle_groups(
                "dependencies { implementation(\"org.example:lib:1.0\"); testImplementation 'org.test:fixture:2' }"
            ),
            ["org.example", "org.test"]
        );
    }
}
