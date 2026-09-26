use crate::core::models::DocSection;
use anyhow::Result;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::Deserialize;
#[derive(Default, Deserialize)]
struct Frontmatter {
    doc_type: Option<String>,
    title: Option<String>,
}
pub fn extract(path: &str, source: &str, mtime: f64) -> Result<Vec<DocSection>> {
    let (meta, body) = if let Some(rest) = source
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
            (meta, body)
        } else {
            (Frontmatter::default(), source)
        }
    } else {
        (Frontmatter::default(), source)
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
    let mut docs = Vec::new();
    for (i, (start, title)) in headings.iter().enumerate() {
        let end = headings.get(i + 1).map_or(body.len(), |(start, _)| *start);
        let content = &body[*start..end];
        if content.trim().is_empty() {
            continue;
        }
        let mut symbols = Vec::new();
        let mut invariants = Vec::new();
        let mut quote = String::new();
        let mut in_quote = false;
        for event in Parser::new(content) {
            match event {
                Event::Code(s) => {
                    let s = s.trim_end_matches("()");
                    if s.chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                        && s.chars().all(|c| c.is_alphanumeric() || "_.:".contains(c))
                    {
                        symbols.push(s.to_owned());
                    }
                    if in_quote {
                        quote.push_str(s);
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
                            .any(|a| quote.contains(a))
                    {
                        invariants.push(quote.trim().to_owned());
                    }
                    in_quote = false;
                }
                Event::Text(t) if in_quote => quote.push_str(&t),
                Event::SoftBreak | Event::HardBreak if in_quote => quote.push('\n'),
                _ => {}
            }
        }
        symbols.sort();
        symbols.dedup();
        docs.push(DocSection {
            doc_id: format!("doc:{path}:{i}"),
            path: path.into(),
            section_title: title.clone(),
            doc_type: meta.doc_type.clone().unwrap_or_else(|| "guide".into()),
            content: content.into(),
            is_invariant: !invariants.is_empty(),
            invariants,
            referenced_symbols: symbols,
            mtime,
            size: content.len() as u64,
        });
    }
    Ok(docs)
}
