use super::*;

#[test]
fn document_references_keep_distinct_sections_and_ambiguous_candidate_sources() {
    let ws = reference_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let overview = client
        .document("docs/reference/mcp-tools.md", json!({"limit":100}))
        .expect("MCP tool should succeed");
    let reference_rows = overview["referenced_symbols"]["items"]
        .as_array()
        .expect("verified code reference rows");
    assert_eq!(reference_rows.len(), 4);
    assert!(
        reference_rows
            .iter()
            .all(|reference| reference["name"] == "code_map_overview")
    );
    for relation in ["cites_doc", "cited_by_doc"] {
        assert_eq!(overview[relation]["total"], 0);
        assert!(overview[relation].get("omitted").is_none());
    }
    let cited_section_ids = reference_rows
        .iter()
        .map(|reference| reference["doc_id"].as_str().expect("citing section id"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(cited_section_ids.len(), 4, "citation section identity");
    assert_eq!(overview["referenced_symbol_candidates"]["total"], 2);
    assert!(
        crate::json_array(&overview["referenced_symbol_candidates"]["items"])
            .iter()
            .all(
                |candidate| candidate["source"]["path"] == "docs/reference/mcp-tools.md"
                    && candidate["source"]["name"] == "shared"
                    && candidate["source"]["line"] == 19
                    && candidate["target"]["path"] == "src/candidates.py"
            )
    );

    let citation_lines = reference_rows
        .iter()
        .map(|reference| reference["citation_line"].as_u64().expect("citation line"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(citation_lines, [7, 10, 13, 16].into_iter().collect());
    assert!(
        reference_rows
            .iter()
            .all(|reference| reference["line"] == 1)
    );
    let target_declaration_uri =
        format!("file://{}#L1)", ws.root().join("src/overview.py").display());
    for detail in [None, Some("compact")] {
        let mut arguments =
            json!({"path_or_id":"docs/reference/mcp-tools.md","limit":100,"format":"markdown"});
        if let Some(detail) = detail {
            arguments["detail"] = json!(detail);
        }
        let markdown = response_text(&mut client, "get_doc", arguments);
        assert_eq!(
            markdown.matches(&target_declaration_uri).count(),
            4,
            "{detail:?}: {markdown}"
        );
        let location = format!("[src/overview.py]({target_declaration_uri} line 1");
        assert_eq!(
            markdown.matches(&location).count(),
            4,
            "{detail:?}: {markdown}"
        );
        for citation_line in [7, 10, 13, 16] {
            assert!(
                markdown.contains(&format!("**citation_line**: `{citation_line}`")),
                "{detail:?}: {markdown}"
            );
        }
        if detail.is_none() {
            assert!(
                !markdown.contains("## Verified document citations"),
                "zero-total citations page has no heading: {markdown}"
            );
            assert!(
                !markdown.contains("## Verified incoming document citations"),
                "zero-total incoming citations page has no heading: {markdown}"
            );
            assert!(markdown.contains("## Verified referenced symbols"));
        } else {
            assert_eq!(
                markdown
                    .matches("Request detail=full for documentary reference candidates")
                    .count(),
                1,
                "compact omission hint: {markdown}"
            );
            assert_eq!(
                markdown
                    .matches("## Ambiguous symbol candidates (navigation only; unverified)")
                    .count(),
                1,
                "compact candidate heading: {markdown}"
            );
        }
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn document_reference_pages_preserve_every_section_in_full_and_compact_detail() {
    let ws = reference_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let target_declaration_uri =
        format!("file://{}#L1)", ws.root().join("src/overview.py").display());
    for detail in ["full", "compact"] {
        let mut offset = 0_u64;
        let mut generation = Value::Null;
        let mut paged_citations = Vec::new();
        loop {
            let mut arguments = json!({
                "path_or_id":"docs/reference/mcp-tools.md",
                "detail":detail,
                "limit":1,
                "offset":offset
            });
            if !generation.is_null() {
                arguments["generation"] = generation.clone();
            }
            let page = client
                .payload("get_doc", arguments.clone())
                .expect("MCP tool should succeed");
            let reference = &page["referenced_symbols"]["items"][0];
            assert_eq!(reference["line"], 1);
            let citation_line = reference["citation_line"].as_u64().unwrap();
            paged_citations.push((
                reference["doc_id"].as_str().unwrap().to_owned(),
                citation_line,
            ));

            arguments["format"] = json!("markdown");
            let markdown = response_text(&mut client, "get_doc", arguments);
            assert_eq!(markdown.matches(&target_declaration_uri).count(), 1);
            assert!(markdown.contains(&format!(
                "[src/overview.py]({target_declaration_uri} line 1"
            )));
            assert!(markdown.contains(&format!("**citation_line**: `{citation_line}`")));

            let reference_page = &page["referenced_symbols"];
            if !reference_page["has_more"].as_bool().unwrap() {
                break;
            }
            offset = reference_page["next_offset"].as_u64().unwrap();
            generation = reference_page["generation"].clone();
        }
        assert_eq!(
            paged_citations
                .iter()
                .map(|(_, line)| line)
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            [7, 10, 13, 16].into_iter().collect()
        );
        assert_eq!(paged_citations.len(), 4);
        assert_eq!(
            paged_citations
                .iter()
                .map(|(doc_id, _)| doc_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4,
            "all paged {detail} citations retain distinct citing sections"
        );
    }
    client.finish().expect("server must finish successfully");
}
