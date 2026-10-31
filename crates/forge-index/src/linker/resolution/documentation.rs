use super::*;

pub(super) fn phase_documentation<'a, 'file, F>(resolver: &mut FileResolver<'a, 'file, F>)
where
    'a: 'file,
    F: AsRef<Facts> + Sync,
{
    let input = resolver.input;
    let path = resolver.path;
    let facts = resolver.facts;
    let all = input.all;
    let by_name = input.by_name;
    let by_qual = input.by_qual;
    let by_suffix = input.by_suffix;
    let graph = &mut resolver.graph;
    let doc_nodes_by_id: HashMap<&str, &Node> = facts
        .nodes
        .iter()
        .filter(|node| matches!(node.kind.as_str(), "document" | "invariant"))
        .map(|node| (node.id.as_str(), node))
        .collect();

    for doc in &facts.docs {
        let document_node = doc_nodes_by_id.get(doc.doc_id.as_str()).copied();
        let document_details = document_node.map(|node| &node.details);
        if let Some(citations) = document_details
            .and_then(|details| details.get("citations"))
            .and_then(serde_json::Value::as_array)
        {
            for citation in citations {
                let Some(target) = citation["target"].as_str() else {
                    continue;
                };
                let line = citation["line"]
                    .as_u64()
                    .unwrap_or(document_node.map_or(1, |node| node.line as u64))
                    as usize;
                let Some((target_path, anchor)) = resolve_document_target(path, target) else {
                    continue;
                };
                let Some(target_facts) = all.get(target_path.as_ref()).map(|facts| facts.as_ref())
                else {
                    continue;
                };
                let matching_sections = target_facts
                    .docs
                    .iter()
                    .filter(|section| {
                        anchor.is_empty() || markdown_slug(&section.section_title) == anchor
                    })
                    .collect::<Vec<_>>();
                if let [target_doc] = matching_sections.as_slice() {
                    for (src, dst, kind) in [
                        (&doc.doc_id, &target_doc.doc_id, "cites_doc"),
                        (&target_doc.doc_id, &doc.doc_id, "cited_by_doc"),
                    ] {
                        graph.push_edge(path, src.clone(), dst.clone(), kind, line, target, "exact");
                    }
                }
            }
        }
        let claims = document_details
            .and_then(|details| details.get("symbol_claims"))
            .and_then(serde_json::Value::as_array);
        let mut references = Vec::new();
        if let Some(claims) = claims.filter(|claims| !claims.is_empty()) {
            references.extend(claims.iter().filter_map(|claim| {
                Some((
                    claim["name"].as_str()?.to_owned(),
                    claim["line"]
                        .as_u64()
                        .unwrap_or(document_node.map_or(1, |node| node.line as u64))
                        as usize,
                    claim["explicit_path"].as_bool().unwrap_or(false),
                    claim["target_path"].as_str().map(str::to_owned),
                ))
            }));
        } else {
            references.extend(doc.referenced_symbols.iter().map(|symbol| {
                (
                    symbol.clone(),
                    document_node.map_or(1, |node| node.line),
                    false,
                    None,
                )
            }));
        }
        for (symbol, occurrence_line, explicit_path, target_path) in references {
            let normalized = symbol.replace("::", ".");
            let path_filter = if explicit_path {
                target_path.as_deref()
            } else {
                None
            };
            if explicit_path && path_filter.is_none() {
                continue;
            }
            let Some(candidates) = document_symbol_candidates(
                &normalized,
                &symbol,
                path,
                path_filter,
                by_qual,
                by_name,
                by_suffix,
            ) else {
                continue;
            };
            if let [node] = candidates.as_slice() {
                for (src, dst, kind) in [
                    (&doc.doc_id, &node.id, "documents"),
                    (&node.id, &doc.doc_id, "references_doc"),
                ] {
                    graph.push_edge(
                        path,
                        src.clone(),
                        dst.clone(),
                        kind,
                        occurrence_line,
                        symbol.clone(),
                        "exact",
                    );
                }
            }
        }
        if let Some(claims) = document_details
            .and_then(|details| details.get("symbol_claims"))
            .and_then(serde_json::Value::as_array)
        {
            for claim in claims {
                let (Some(name), Some(target_path)) =
                    (claim["name"].as_str(), claim["target_path"].as_str())
                else {
                    continue;
                };
                if target_path.split('/').any(|component| component == "..") {
                    continue;
                }
                let target_path = target_path.to_owned();
                let normalized = name.replace("::", ".");
                let Some(candidates) = document_symbol_candidates(
                    &normalized,
                    name,
                    path,
                    Some(&target_path),
                    by_qual,
                    by_name,
                    by_suffix,
                ) else {
                    continue;
                };
                let [target_node] = candidates.as_slice() else {
                    continue;
                };
                let claim_line = claim["line"].as_u64().unwrap_or(1) as usize;
                let invariant_nodes = facts
                    .nodes
                    .iter()
                    .filter(|node| node.kind == "invariant" && node.line == claim_line)
                    .collect::<Vec<_>>();
                for invariant in invariant_nodes {
                    for (src, dst, kind) in [
                        (&invariant.id, &target_node.id, "governs"),
                        (&target_node.id, &invariant.id, "governed_by"),
                    ] {
                        graph.push_edge(
                            path,
                            src.clone(),
                            dst.clone(),
                            kind,
                            claim_line,
                            name,
                            "exact",
                        );
                    }
                }
            }
        }
    }
}

fn resolve_document_target<'a>(
    source_path: &'a str,
    target: &str,
) -> Option<(Cow<'a, str>, String)> {
    if target.starts_with('/') || target.contains("\\") || target.contains('?') {
        return None;
    }
    let (target_path, anchor) = target.split_once('#').unwrap_or((target, ""));
    let (workspace_prefix, source_relative) = if source_path.starts_with('[') {
        let close = source_path.find("]/")? + 2;
        (&source_path[..close], &source_path[close..])
    } else {
        ("", source_path)
    };
    if target_path.is_empty() {
        return Some((Cow::Borrowed(source_path), markdown_slug(anchor)));
    }
    let mut normalized = String::with_capacity(
        workspace_prefix.len() + source_relative.len() + target_path.len() + 3,
    );
    normalized.push_str(workspace_prefix);
    let prefix_len = normalized.len();
    for component in std::path::Path::new(source_relative).parent()?.components() {
        let component = component.as_os_str().to_str()?;
        if normalized.len() > prefix_len && !normalized.ends_with('/') {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    for component in target_path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if normalized.len() <= prefix_len {
                    return None;
                }
                if let Some(separator) = normalized.rfind('/').filter(|&index| index >= prefix_len)
                {
                    normalized.truncate(separator);
                } else {
                    normalized.truncate(prefix_len);
                }
            }
            value if value.contains(':') => return None,
            value => {
                if !normalized.is_empty() && !normalized.ends_with('/') {
                    normalized.push('/');
                }
                normalized.push_str(value);
            }
        }
    }
    if normalized.len() == prefix_len {
        return None;
    }
    if std::path::Path::new(&normalized).extension().is_none() {
        normalized.push_str(".md");
    }
    Some((Cow::Owned(normalized), markdown_slug(anchor)))
}

fn markdown_slug(anchor: &str) -> String {
    anchor
        .trim_start_matches('#')
        .chars()
        .filter_map(|character| {
            if character.is_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character.is_whitespace() || character == '-' {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}
