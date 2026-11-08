use super::*;

#[test]
fn cli_and_mcp_test_mapping_preserve_verified_call_paths() {
    let ws = document_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let mcp_text = response_text(
        &mut client,
        "code_map_tests",
        json!({"selector":"main.py:target","direction":"inbound","format":"markdown"}),
    );
    let cli_text = ws
        .cli_text(&["query", "tests", "main.py:target"])
        .expect("CLI should succeed");
    for (caller, text) in [("mcp", mcp_text), ("cli", cli_text)] {
        let breadcrumb = text
            .split("Verified call path:")
            .nth(1)
            .expect("verified test-to-target path");
        let test_case = breadcrumb.find("test_target").expect("case node");
        let helper = breadcrumb.find("helper_one").expect("helper node");
        let target = breadcrumb.rfind("target").expect("target node");
        assert!(
            test_case < helper && helper < target,
            "{caller}: {breadcrumb}"
        );
        for token in ["test_case", "test_helper", "(target)", "file://", "#L"] {
            assert!(breadcrumb.contains(token), "{caller} {token}: {breadcrumb}");
        }
    }
    let cli = ws
        .query(&["query", "tests", "main.py:target"])
        .expect("CLI should succeed");
    let mcp = client
        .payload(
            "code_map_tests",
            json!({"selector":"main.py:target","direction":"inbound"}),
        )
        .expect("MCP tool should succeed");
    for pointer in [
        "/nodes/items/0/connection/call_path",
        "/nodes/items/0/connection/verified_call_path_nodes",
        "/nodes/items/0/test_role",
    ] {
        assert_eq!(cli.pointer(pointer), mcp.pointer(pointer), "{pointer}");
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn document_detail_preserves_full_content_and_compact_outline_across_formats() {
    let ws = document_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let document = client
        .document("README.md", json!({}))
        .expect("MCP tool should succeed");
    assert!(document["sections"]["items"][0]["content"]
        .as_str()
        .unwrap()
        .contains("documented contract"));
    let outline = client
        .document("README.md", json!({"detail":"compact"}))
        .expect("MCP tool should succeed");
    assert!(outline["sections"]["items"][0].get("content").is_none());
    assert_eq!(document["sections"]["total"], outline["sections"]["total"]);
    let full_markdown = response_text(
        &mut client,
        "get_doc",
        json!({"path_or_id":"README.md","format":"markdown"}),
    );
    assert!(full_markdown.contains("Document sections"));
    assert!(full_markdown.contains("Source Markdown"));
    assert!(full_markdown.contains("documented contract"));
    assert!(full_markdown.contains("[README.md](file://"));
    let outline_markdown = response_text(
        &mut client,
        "get_doc",
        json!({"path_or_id":"README.md","detail":"compact","format":"markdown"}),
    );
    assert!(outline_markdown.contains("Document sections"));
    assert!(!outline_markdown.contains("Source Markdown"));
    assert_eq!(
        outline_markdown
            .matches("Request detail=full for documentary reference candidates")
            .count(),
        1,
        "compact output prints the candidate omission hint once: {outline_markdown}"
    );
    assert_eq!(
        outline_markdown
            .matches("## Ambiguous symbol candidates (navigation only; unverified)")
            .count(),
        1,
        "compact output uses one candidate heading: {outline_markdown}"
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn document_search_markdown_preserves_page_metadata_excerpts_and_section_identity() {
    let ws = document_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let markdown = response_text(
        &mut client,
        "search_docs",
        json!({"query":"documented contract","include_excerpt":true,"format":"markdown"}),
    );
    let json = client
        .payload(
            "search_docs",
            json!({"query":"documented contract","include_excerpt":true}),
        )
        .expect("MCP tool should succeed");
    let section = &json["sections"]["items"][0];
    assert_eq!(markdown.lines().next(), Some("# search_docs"));
    for field in [
        "generation",
        "has_more",
        "limit",
        "next_offset",
        "offset",
        "total",
        "doc_id",
        "doc_type",
        "is_invariant",
        "rank",
        "section_title",
        "size",
    ] {
        assert!(
            markdown.contains(&format!("**{field}**:")),
            "{field}: {markdown}"
        );
    }
    for token in [
        "Search results",
        "## Sections",
        "**has_more**: `false`",
        "**limit**: `30`",
        "**next_offset**: `null`",
        "**offset**: `0`",
        "**total**: `1`",
        "Match excerpt",
        "# [Contract]\n\nThe `target` function preserves the [documented] [contract].",
        "[README.md](file://",
    ] {
        assert!(markdown.contains(token), "{token}: {markdown}");
    }
    assert!(markdown.contains(&format!(
        "### {}",
        section["section_title"].as_str().unwrap()
    )));
    for field in ["doc_type", "is_invariant", "section_title"] {
        let token = format!("**{field}**: `{}`", section.get(field).unwrap());
        assert!(markdown.contains(&token), "{token}: {markdown}");
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn document_markdown_admits_edited_sources_and_keeps_freshness_evidence() {
    let (ws, harness) = McpTestHarness::with_files(DOCUMENT_FILES);
    let mut client = harness.expect("MCP initialize");
    client
        .document("README.md", json!({}))
        .expect("MCP tool should succeed");
    ws.write(
        "README.md",
        "# Contract\n\nThe `target` function preserves the documented contract.\nFreshly refreshed documentation evidence.\n",
    );
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let refreshed_markdown = response_text(
        &mut client,
        "get_doc",
        json!({"path_or_id":"README.md","format":"markdown"}),
    );
    assert!(refreshed_markdown.contains("Freshly refreshed documentation evidence."));
    for token in [
        "- **freshness**\n",
        "  - **corpus_hash**:",
        "  - **files_checked**:",
        "  - **output_root**:",
        "  - **refresh**: `\"delta\"`",
        "  - **status**: `\"source_inventory_matched\"`",
    ] {
        assert!(
            refreshed_markdown.contains(token),
            "missing source-admitted freshness token {token}: {refreshed_markdown}"
        );
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn explain_document_controls_and_direction_aliases_preserve_the_graph() {
    let ws = document_workspace();
    let mut client = McpTestHarness::new(ws).expect("MCP initialize");
    let mut explain = |arguments: Value| {
        client
            .payload("code_map_explain", arguments)
            .expect("MCP tool should succeed")
    };
    let with_docs = explain(json!({"selector":"target"}));
    assert!(with_docs["documents"]["total"].as_u64().unwrap() > 0);
    let without_docs = explain(json!({"selector":"target","show_doc":false}));
    assert!(without_docs.get("documents").is_none() || without_docs["documents"]["total"] == 0);
    assert_eq!(with_docs["coverage"], without_docs["coverage"]);
    assert_eq!(with_docs["incoming"], without_docs["incoming"]);
    for (alias, canonical, omitted) in [
        ("inbound", "incoming", "outgoing"),
        ("outbound", "outgoing", "incoming"),
    ] {
        let alias_result = explain(json!({"selector":"target","direction":alias}));
        let canonical_result = explain(json!({"selector":"target","direction":canonical}));
        assert_eq!(alias_result["direction"], canonical);
        assert_eq!(alias_result[canonical], canonical_result[canonical]);
        assert_eq!(alias_result[omitted]["omitted"], true);
    }
    client.finish().expect("server must finish successfully");
}
