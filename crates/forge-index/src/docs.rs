use crate::core::models::DocSection;
use anyhow::Result;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Default, Deserialize)]
struct Frontmatter {
    doc_type: Option<String>,
    title: Option<String>,
    status: Option<String>,
}

struct ActiveLink {
    line: usize,
    target_path: Option<String>,
    explicit_path: bool,
    range: Option<(usize, usize)>,
    occurrence: usize,
    claim_start: usize,
}

/// Source-derived metadata retained on the generic document node.
pub(crate) fn extract_with_details(
    path: &str,
    source: &str,
    mtime: f64,
) -> Result<Vec<(DocSection, Value)>> {
    let (meta, body, body_offset) = if let Some(rest) = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
    {
        if let Some(end) = rest.find("\n---") {
            let meta = serde_yaml::from_str::<Frontmatter>(&rest[..end]).unwrap_or_default();
            let after = &rest[end + 4..];
            let body = after
                .strip_prefix('\n')
                .or_else(|| after.strip_prefix("\r\n"))
                .unwrap_or(after);
            let offset = source.len() - body.len();
            (meta, body, offset)
        } else {
            (Frontmatter::default(), source, 0)
        }
    } else {
        (Frontmatter::default(), source, 0)
    };

    let mut headings = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => current = Some((range.start, String::new())),
            Event::Text(t) | Event::Code(t) if current.is_some() => {
                if let Some((_, title)) = &mut current {
                    title.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((start, title)) = current.take() {
                    headings.push((start, title));
                }
            }
            _ => {}
        }
    }
    if headings.first().is_none_or(|(start, _)| *start > 0) {
        headings.insert(0, (0, meta.title.clone().unwrap_or_else(|| path.into())));
    }

    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let mut output = Vec::new();
    for (index, (start, title)) in headings.iter().enumerate() {
        let end = headings
            .get(index + 1)
            .map_or(body.len(), |(start, _)| *start);
        let content = &body[*start..end];
        if content.trim().is_empty() {
            continue;
        }
        let line_at = |offset: usize| -> usize {
            let pos = body_offset + *start + offset;
            line_starts.partition_point(|&start_pos| start_pos <= pos)
        };
        let section_line = line_at(0);
        let section_end_line = section_line + content.lines().count().saturating_sub(1);

        let mut symbols = Vec::new();
        let mut invariants = Vec::new();
        let mut citations = Vec::new();
        let mut symbol_claims: Vec<Value> = Vec::new();
        let mut quote = String::new();
        let mut in_quote = false;
        let mut active_link: Option<ActiveLink> = None;
        let mut next_link_occurrence = 0usize;
        for (event, range) in Parser::new(content).into_offset_iter() {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    let line = line_at(range.start);
                    let target = dest_url.to_string();
                    citations.push(json!({"target":target,"line":line}));
                    let raw_path = target.split('#').next().unwrap_or_default();
                    let explicit_path = !raw_path.is_empty();
                    let target_path = explicit_path
                        .then(|| normalize_claim_path(path, raw_path))
                        .flatten();
                    let range = line_anchor(&target).and_then(|(raw_path, first, last)| {
                        normalize_claim_path(path, &raw_path)
                            .map(|target_path| (target_path, first, last))
                    });
                    active_link = Some(ActiveLink {
                        line,
                        target_path,
                        explicit_path,
                        range: range.map(|(_, first, last)| (first, last)),
                        occurrence: next_link_occurrence,
                        claim_start: symbol_claims.len(),
                    });
                    next_link_occurrence = next_link_occurrence.saturating_add(1);
                }
                Event::End(TagEnd::Link) => {
                    if let Some(link) = active_link.take() {
                        let label_count = symbol_claims.len().saturating_sub(link.claim_start);
                        for claim in &mut symbol_claims[link.claim_start..] {
                            claim["label_count"] = json!(label_count);
                        }
                    }
                }
                Event::Code(value) => {
                    let name = value.trim_end_matches("()").trim();
                    if is_symbol_token(name) {
                        symbols.push(name.to_owned());
                        let mut claim = json!({
                            "name":name,
                            "line":line_at(range.start),
                            "explicit_path":active_link.as_ref().is_some_and(|link| link.explicit_path),
                            "label_count":1,
                        });
                        if let Some(link) = &active_link {
                            claim["link_occurrence"] = json!(link.occurrence);
                            claim["line"] = json!(link.line);
                            if let Some(target_path) = &link.target_path {
                                claim["target_path"] = json!(target_path);
                            }
                            if let Some((first, last)) = link.range {
                                claim["start_line"] = json!(first);
                                claim["end_line"] = json!(last);
                            }
                        }
                        symbol_claims.push(claim);
                    }
                    if in_quote {
                        quote.push_str(&value);
                    }
                }
                Event::Start(Tag::BlockQuote) => {
                    in_quote = true;
                    quote.clear();
                }
                Event::End(TagEnd::BlockQuote) => {
                    if quote.contains("Invariant:")
                        && ["[!IMPORTANT]", "[!WARNING]", "[!NOTE]"]
                            .iter()
                            .any(|alert| quote.contains(alert))
                    {
                        invariants.push(quote.trim().to_owned());
                    }
                    in_quote = false;
                }
                Event::Text(value) if in_quote => quote.push_str(&value),
                Event::SoftBreak | Event::HardBreak if in_quote => quote.push('\n'),
                _ => {}
            }
        }
        append_wikilinks(content, section_line, &mut citations);
        symbols.sort();
        symbols.dedup();

        let doc_id = format!("doc:{path}:{index}");
        let doc = DocSection {
            doc_id: doc_id.clone(),
            path: path.into(),
            section_title: title.clone(),
            doc_type: meta.doc_type.clone().unwrap_or_else(|| "guide".into()),
            content: content.into(),
            is_invariant: !invariants.is_empty()
                || content.lines().any(|line| invariant_id(line).is_some()),
            invariants: invariants.clone(),
            referenced_symbols: symbols,
            mtime,
            size: content.len() as u64,
        };
        let declared_invariants = content
            .lines()
            .enumerate()
            .filter_map(|(offset, line)| {
                let id = invariant_id(line)?;
                let rule = line.trim().trim_start_matches(['-', '*']).trim();
                (!rule.is_empty())
                    .then(|| json!({"id":id,"rule":rule,"line":section_line + offset}))
            })
            .collect::<Vec<_>>();
        output.push((
            doc,
            json!({
                "line":section_line,
                "end_line":section_end_line,
                "status":meta.status,
                "citations":citations,
                "symbol_claims":symbol_claims,
                "declared_invariants":declared_invariants,
                "is_invariant":!invariants.is_empty(),
            }),
        ));
    }
    Ok(output)
}

/// Performs extract.
pub fn extract(path: &str, source: &str, mtime: f64) -> Result<Vec<DocSection>> {
    Ok(extract_with_details(path, source, mtime)?
        .into_iter()
        .map(|(doc, _)| doc)
        .collect())
}

fn is_symbol_token(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || "_.:".contains(c))
}

fn line_anchor(target: &str) -> Option<(String, usize, usize)> {
    let (path, fragment) = target.split_once('#')?;
    let fragment = fragment.strip_prefix('L')?;
    let (first, last) = fragment.split_once("-L").unwrap_or((fragment, fragment));
    let first = first.parse::<usize>().ok()?;
    let last = last.parse::<usize>().ok()?;
    (first > 0 && last >= first && !path.is_empty()).then(|| (path.to_owned(), first, last))
}

fn normalize_claim_path(source_path: &str, target_path: &str) -> Option<String> {
    if target_path.is_empty()
        || target_path.starts_with('/')
        || target_path.contains('\\')
        || target_path.contains(':')
    {
        return None;
    }
    let (workspace_prefix, source_relative) = if source_path.starts_with('[') {
        let close = source_path.find("]/")? + 2;
        (&source_path[..close], &source_path[close..])
    } else {
        ("", source_path)
    };
    let mut components = std::path::Path::new(source_relative)
        .parent()?
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    for component in target_path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            value => components.push(value.to_owned()),
        }
    }
    (!components.is_empty()).then(|| format!("{workspace_prefix}{}", components.join("/")))
}

fn invariant_id(line: &str) -> Option<&str> {
    let start = line.find("INV-")?;
    let rest = &line[start..];
    let (id, _) = rest.split_once(':')?;
    (id.len() > 4
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        }))
    .then_some(id)
}

fn append_wikilinks(content: &str, start_line: usize, citations: &mut Vec<Value>) {
    for (line_offset, line) in content.lines().enumerate() {
        let mut rest = line;
        let mut consumed = 0;
        while let Some(open) = rest.find("[[") {
            let after_open = open + 2;
            let Some(close) = rest[after_open..].find("]]") else {
                break;
            };
            let target = rest[after_open..after_open + close].trim();
            if !target.is_empty() {
                citations
                    .push(json!({"target":target,"line":start_line + line_offset,"wiki":true}));
            }
            let advance = after_open + close + 2;
            consumed += advance;
            rest = &line[consumed..];
        }
    }
}
