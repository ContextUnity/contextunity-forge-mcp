use super::*;

#[test]
fn mcp_document_governance_inspection_and_reference_identity() {
    let (ws, harness) = McpTestHarness::with_files(DOCUMENT_FILES);
    drop(harness.expect("MCP initialize"));
    ws.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs/allowed, docs/reference]\n",
    );
    ws.write(
        "src/main.rs",
        "pub fn target() {}\npub fn caller() { target(); }\npub fn grandcaller() { caller(); }\n/// Full detail documentation from node.details.\npub fn documented() {\n    helper();\n    helper();\n}\npub fn helper() {}\n",
    );
    ws.write("src/other.rs", "pub fn documented() {}\n");
    ws.write("src/ambiguous.rs", "pub struct A;\nimpl A { pub fn shared() {} }\npub struct B;\nimpl B { pub fn shared() {} }\n");
    ws.write(
        "AGENTS.md",
        "# Agent rules\n- INV-AGENT: Preserve [`target`](src/main.rs#L1).\n",
    );
    ws.write("docs/guide.md", "---\ntitle: Navigation\ndoc_type: guide\n---\n# Navigation\n[Safety](rules.md#safety)\n[[extra#Other]]\n[Missing](missing.md)\n[Bad anchor](rules.md#absent)\n[Escape](../../outside.md)\n[[shared]]\n");
    let mut rules = String::from(
        "---\ntitle: Accepted rules\ndoc_type: adr\nstatus: accepted\n---\n# Safety\n",
    );
    for index in 0..6 {
        rules.push_str(&format!("- INV-D{index:02}: Preserve target contract {index} through [`target`](../src/main.rs#L1).\n"));
    }
    rules.push_str("[Navigation](guide.md#navigation)\n");
    ws.write("docs/rules.md", &rules);
    let mut indirect = String::from("# Other\n");
    for index in 0..6 {
        indirect.push_str(&format!("- INV-T{index:02}: Preserve caller contract {index} through [`caller`](../src/main.rs#L2).\n"));
    }
    indirect.push_str("- INV-DEEP: Preserve [`grandcaller`](../src/main.rs#L3).\n");
    ws.write("docs/extra.md", &indirect);
    ws.write("docs/left/shared.md", "# Shared\nLeft candidate.\n");
    ws.write("docs/right/shared.md", "# Shared\nRight candidate.\n");
    ws.write("docs/plans/proposed.md", "---\ntitle: Proposal\ndoc_type: plan\nstatus: proposed\n---\n# Proposal\n- INV-PLAN: Proposed [`target`](../../src/main.rs#L1) rule.\n");
    ws.write("docs/milestones/010-navigation.md", "---\nid: m-navigation\ntitle: Navigation contract\ndoc_type: contract\nstatus: active\n---\n# Contract\n- INV-MILESTONE: Preserve [`target`](../../src/main.rs#L1).\n");
    ws.write(
        "docs/unknown.md",
        "# Unspecified status\n- INV-UNKNOWN: Preserve [`target`](../src/main.rs#L1).\n",
    );
    ws.write("docs/range.md", "# Range\n[`target`](../src/main.rs#L1)\n[generic label](../src/main.rs#L2)\n[`target`](../src/main.rs#L0)\n[`target`](../src/main.rs#L999)\n[`shared`](../src/ambiguous.rs#L1)\n");
    ws.write(
        "docs/allowed/inspect.md",
        "---\ntitle: Inspect context\ndoc_type: guide\n---\n# Inspect context\nUnique full inspect document content.\n[`documented`](../../src/main.rs#L5)\n",
    );
    ws.build();
    let mut client = McpTestHarness::new(&ws).expect("MCP initialize");

    let guide = client
        .document("docs/guide.md", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert_eq!(guide["sections"]["items"][0]["line"], 5);
    let cited = guide["cites_doc"]["items"]
        .as_array()
        .expect("verified document citations");
    let paths = cited
        .iter()
        .map(|section| section["path"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        ["docs/rules.md", "docs/extra.md"].into_iter().collect()
    );
    let reciprocal = client
        .document("docs/rules.md", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert!(
        crate::json_array(&reciprocal["cited_by_doc"]["items"])
            .iter()
            .any(|section| section["path"] == "docs/guide.md")
    );
    let edges = client.payload("code_map_analyze", json!({"target":"SELECT e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind IN('cites_doc','cited_by_doc') AND p.path='docs/guide.md' ORDER BY e.kind,e.line","limit":100})).expect("MCP tool should succeed");
    assert!(
        crate::json_array(&edges["rows"]["items"])
            .iter()
            .any(|edge| edge["kind"] == "cites_doc" && edge["line"] == 6)
    );

    for (path, doc_type, status) in [
        ("AGENTS.md", "guide", Value::Null),
        ("docs/plans/proposed.md", "plan", json!("proposed")),
        (
            "docs/milestones/010-navigation.md",
            "contract",
            json!("active"),
        ),
        ("docs/unknown.md", "guide", Value::Null),
    ] {
        let document = client
            .payload("get_doc", json!({"path_or_id":path,"limit":100}))
            .expect("MCP tool should succeed");
        let section = &document["sections"]["items"][0];
        assert_eq!(section["doc_type"], doc_type);
        assert_eq!(
            section["status"], status,
            "unspecified is not accepted: {section}"
        );
        assert!(
            crate::json_array(&document["referenced_symbols"]["items"])
                .iter()
                .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/main.rs")
        );
    }
    let invariant_nodes = client.payload("code_map_analyze", json!({"target":"SELECT n.name,p.path,n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='invariant' ORDER BY n.name,p.path,n.line","limit":100})).expect("MCP tool should succeed");
    assert!(
        crate::json_array(&invariant_nodes["rows"]["items"])
            .iter()
            .any(|node| node["name"] == "INV-PLAN"
                && node["path"] == "docs/plans/proposed.md"
                && node["line"] == 7)
    );
    let governance = client.payload("code_map_analyze", json!({"target":"SELECT e.kind,s.name source_name,t.name target_name,p.path,e.line FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes t ON t.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind IN('governs','governed_by') ORDER BY e.kind,s.name,t.name","limit":100})).expect("MCP tool should succeed");
    let governance = crate::json_array(&governance["rows"]["items"]);
    for (kind, source, target) in [
        ("governs", "INV-D00", "target"),
        ("governed_by", "target", "INV-D00"),
    ] {
        assert!(governance.iter().any(|edge| edge["kind"] == kind
            && edge["source_name"] == source
            && edge["target_name"] == target
            && edge["path"] == "docs/rules.md"
            && edge["line"] == 7));
    }
    let range = client
        .document("docs/range.md", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert!(
        crate::json_array(&range["referenced_symbols"]["items"])
            .iter()
            .all(|symbol| symbol["name"] != "shared"),
        "ambiguous symbol claims are not verified references"
    );
    let governed = client.payload("code_map_analyze", json!({"target":"SELECT count(*) total FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE e.kind='governs' AND n.name='shared'"})).expect("MCP tool should succeed");
    assert_eq!(governed["rows"]["items"][0]["total"], 0);

    for tool in ["code_map_inspect", "code_map_impact"] {
        let mut arguments = json!({"selector":"src/main.rs:target","limit":100});
        if tool == "code_map_inspect" {
            arguments["show_source"] = json!(false);
        }
        let result = client
            .payload(tool, arguments)
            .expect("MCP tool should succeed");
        let invariants = &result["governing_invariants"];
        let direct = crate::json_array(&invariants["direct"]);
        assert_eq!(direct.len(), 5);
        assert_eq!(
            direct
                .iter()
                .map(|rule| rule["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["INV-AGENT", "INV-D00", "INV-D01", "INV-D02", "INV-D03"]
        );
        assert_eq!(invariants["omitted_direct"], 5);
        for rule in direct {
            assert!(rule["rule"].as_str().unwrap().contains("Preserve"));
            assert!(!rule["rule"].as_str().unwrap().contains('\n'));
            assert!(rule["source"]["path"].is_string());
            assert!(rule["source"]["line"].as_u64().unwrap() > 0);
        }
        assert_eq!(
            invariants["transitive_ids"],
            json!(["INV-T00", "INV-T01", "INV-T02", "INV-T03", "INV-T04"])
        );
        assert_eq!(invariants["omitted_transitive"], 1);
        assert!(
            !invariants.to_string().contains("INV-DEEP"),
            "one hop is not arbitrary transitive closure"
        );
        let omitted = crate::json_array(&invariants["omitted_sources"]);
        assert!(!omitted.is_empty() && omitted.len() <= 5);
        for reference in omitted {
            assert!(reference["id"].is_string());
            let document = client
                .payload("get_doc", json!({"path_or_id":reference["source"]["path"]}))
                .expect("MCP tool should succeed");
            assert!(document["sections"]["total"].as_u64().unwrap() > 0);
        }
        let docs = &result[if tool == "code_map_inspect" {
            "documented_in"
        } else {
            "affected_docs"
        }];
        assert!(
            crate::json_array(&docs["items"])
                .iter()
                .any(|doc| doc["path"] == "docs/rules.md")
        );
        if tool == "code_map_inspect" {
            let markdown = response_text(
                &mut client,
                tool,
                json!({"selector":"src/main.rs:target","limit":100,"show_source":false,"format":"markdown"}),
            );
            assert!(
                markdown.contains("> [!IMPORTANT] Governing Invariants"),
                "verified rules need their admitted callout: {markdown}"
            );
            for rule in direct {
                assert!(markdown.contains(rule["id"].as_str().unwrap()));
                assert!(markdown.contains(rule["rule"].as_str().unwrap()));
                let source = &rule["source"];
                let coordinate = format!(
                    "file://{}#L{}",
                    ws.root().join(source["path"].as_str().unwrap()).display(),
                    source["line"].as_u64().unwrap()
                );
                assert!(
                    markdown.contains(&coordinate),
                    "missing real rule source {coordinate}: {markdown}"
                );
            }
            for id in crate::json_array(&invariants["transitive_ids"]) {
                assert!(
                    markdown.contains(id.as_str().unwrap()),
                    "one-hop provenance must survive"
                );
            }
            assert!(
                markdown.contains("Omitted one-hop invariant references: 1"),
                "bounded one-hop omission count must remain visible: {markdown}"
            );
            assert!(markdown.contains("### One-hop invariant source references"));
            for reference in omitted {
                let id = reference["id"].as_str().unwrap();
                let source = &reference["source"];
                let path = source["path"].as_str().unwrap();
                let line = source["line"].as_u64().unwrap();
                let coordinate = format!("file://{}#L{}", ws.root().join(path).display(), line);
                let source_row = format!("- `{id}`: [{path}#L{line}]({coordinate})");
                assert!(
                    markdown.contains(&source_row),
                    "missing producer-bound one-hop source {source_row}: {markdown}"
                );
            }
            assert!(!markdown.contains("INV-DEEP"));
        }
    }

    let explain_markdown = response_text(
        &mut client,
        "code_map_explain",
        json!({
            "selector":"src/main.rs:target",
            "direction":"incoming",
            "format":"markdown",
            "limit":100
        }),
    );
    assert!(
        explain_markdown.contains("[proven call]"),
        "{explain_markdown}"
    );
    assert!(
        explain_markdown.contains("src/main.rs:2"),
        "{explain_markdown}"
    );

    let impact_markdown = response_text(
        &mut client,
        "code_map_impact",
        json!({
            "selector":"src/main.rs:target",
            "depth":1,
            "format":"markdown",
            "limit":100
        }),
    );
    assert!(
        impact_markdown.contains("[proven call]"),
        "{impact_markdown}"
    );
    assert!(
        impact_markdown.contains("src/main.rs:2"),
        "{impact_markdown}"
    );
    assert!(
        impact_markdown.contains("> [!IMPORTANT] Governing Invariants"),
        "{impact_markdown}"
    );

    let inspect_arguments = json!({
        "selector":"documented",
        "path":"src/main.rs",
        "detail":"full",
        "show_doc":true,
        "include_coverage":true,
        "show_source":false,
        "limit":100
    });
    let mut inspect_json_arguments = inspect_arguments.clone();
    inspect_json_arguments["format"] = json!("json");
    let inspected = client
        .payload("code_map_inspect", inspect_json_arguments.clone())
        .expect("MCP tool should succeed");
    let node = &inspected["node"];
    assert_eq!(node["path"], "src/main.rs");
    assert_eq!(node["line"], 5);
    let node_id = node["id"].as_str().expect("selected node id");
    let details = &node["details"];
    let signature = details["signature"].as_str().expect("full node signature");
    let docstring = details["doc"].as_str().expect("full node documentation");
    assert!(
        signature.contains("documented")
            && docstring.contains("Full detail documentation from node.details"),
        "{details}"
    );
    let documents = &inspected["documents"];
    assert_eq!(
        documents["has_more"], false,
        "document page should be complete: {documents}"
    );
    let document = documents["items"]
        .as_array()
        .expect("full documentation page")
        .iter()
        .find(|item| item["path"] == "docs/allowed/inspect.md")
        .expect("linked inspect document");
    assert!(
        document["content"]
            .as_str()
            .expect("full document content")
            .contains("Unique full inspect document content")
    );

    let coverage = &inspected["coverage"];
    assert_eq!(
        coverage["has_more"], false,
        "coverage page should be complete: {coverage}"
    );
    assert!(
        coverage["items"]
            .as_array()
            .expect("full coverage page")
            .iter()
            .any(|item| item["expression"] == "helper" && item["status"] == "resolved")
    );
    let resolved_total = coverage["statuses"]
        .as_array()
        .expect("scope-wide coverage statuses")
        .iter()
        .find(|status| status["status"] == "resolved")
        .and_then(|status| status["total"].as_u64())
        .expect("resolved scope count");
    assert!(resolved_total > 0);

    let coverage_total = coverage["total"].as_u64().expect("coverage page total");
    let mut coverage_tail_json_arguments = inspect_json_arguments.clone();
    coverage_tail_json_arguments["limit"] = json!(1);
    coverage_tail_json_arguments["offset"] = json!(coverage_total);
    coverage_tail_json_arguments["generation"] = coverage["generation"].clone();
    let coverage_tail_json = client
        .payload("code_map_inspect", coverage_tail_json_arguments)
        .expect("MCP tool should succeed");
    assert!(
        coverage_tail_json["coverage"]["items"]
            .as_array()
            .expect("empty coverage tail")
            .is_empty()
    );
    assert_eq!(
        coverage_tail_json["coverage"]["statuses"],
        coverage["statuses"]
    );

    let mut coverage_tail_markdown_arguments = inspect_arguments.clone();
    coverage_tail_markdown_arguments["limit"] = json!(1);
    coverage_tail_markdown_arguments["offset"] = json!(coverage_total);
    coverage_tail_markdown_arguments["generation"] = coverage["generation"].clone();
    let coverage_tail_markdown = response_text(
        &mut client,
        "code_map_inspect",
        coverage_tail_markdown_arguments,
    );
    assert!(
        coverage_tail_markdown.contains("## coverage")
            && coverage_tail_markdown.contains("\"statuses\"")
            && coverage_tail_markdown.contains(&format!("\"total\": {resolved_total}")),
        "coverage-wide status totals must survive an empty item page: {coverage_tail_markdown}"
    );

    let mut nested_page_arguments = inspect_json_arguments.clone();
    nested_page_arguments["limit"] = json!(1);
    let nested_pages = client
        .payload("code_map_inspect", nested_page_arguments)
        .expect("MCP tool should succeed");
    let nested_page_key = ["documents", "coverage"]
        .into_iter()
        .find(|key| nested_pages[key]["has_more"] == true)
        .expect("fixture should provide an active nested inspect page");
    let nested_page = &nested_pages[nested_page_key];
    let nested_generation = nested_page["generation"]
        .as_str()
        .expect("nested page generation");
    let nested_offset = nested_page["next_offset"]
        .as_u64()
        .expect("nested page next offset");

    let mut nested_markdown_arguments = inspect_arguments.clone();
    nested_markdown_arguments["limit"] = json!(1);
    let nested_markdown = response_text(&mut client, "code_map_inspect", nested_markdown_arguments);
    let nested_continuation = nested_markdown
        .lines()
        .find(|line| line.starts_with(&format!("Continue {nested_page_key} ")))
        .expect("active nested page continuation in Markdown");
    assert!(
        nested_continuation.contains("path=src/main.rs")
            && nested_continuation.contains("selector=documented")
            && nested_continuation.contains(&format!("offset {nested_offset}"))
            && nested_continuation.contains(&format!("generation `{nested_generation}`")),
        "nested continuation must retain its own scope and accepted token: {nested_continuation}"
    );

    let mut nested_json_continuation_arguments = inspect_json_arguments.clone();
    nested_json_continuation_arguments["limit"] = json!(1);
    nested_json_continuation_arguments["offset"] = json!(nested_offset);
    nested_json_continuation_arguments["generation"] = json!(nested_generation);
    let nested_continued = client
        .payload("code_map_inspect", nested_json_continuation_arguments)
        .expect("MCP tool should succeed");
    assert_eq!(nested_continued["node"]["id"], node_id);
    assert_eq!(nested_continued["node"]["path"], "src/main.rs");
    let continued_page = &nested_continued[nested_page_key];
    assert_eq!(continued_page["offset"], nested_offset);
    assert_eq!(continued_page["generation"], nested_generation);
    assert!(!crate::json_array(&continued_page["items"]).is_empty());
    assert_ne!(
        continued_page["items"][0], nested_page["items"][0],
        "the nested continuation should return the next item"
    );

    let inspect_markdown =
        response_text(&mut client, "code_map_inspect", inspect_arguments.clone());
    assert!(
        inspect_markdown.contains(signature)
            && inspect_markdown.contains(docstring)
            && inspect_markdown.contains("docs/allowed/inspect.md")
            && inspect_markdown.contains("Unique full inspect document content")
            && inspect_markdown.contains("\"expression\": \"helper\"")
            && inspect_markdown.contains("\"status\": \"resolved\"")
            && inspect_markdown.contains("\"statuses\"")
            && inspect_markdown.contains(&format!("\"total\": {resolved_total}")),
        "full node details, document data, and coverage scope must survive inspect Markdown: {inspect_markdown}"
    );

    let expected = client
        .document("docs/rules.md", json!({}))
        .expect("MCP tool should succeed");
    let cli = ws
        .query(&["docs", "get", "docs/rules.md"])
        .expect("CLI should succeed");
    for response in [&cli, &expected] {
        assert!(
            crate::json_array(&response["referenced_symbols"]["items"])
                .iter()
                .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/main.rs")
        );
    }
    assert_eq!(
        cli["referenced_symbols"]["total"],
        expected["referenced_symbols"]["total"]
    );

    let broken = client
        .payload(
            "code_map_analyze",
            json!({"target":"docs/guide.md","limit":100}),
        )
        .expect("MCP tool should succeed");
    let diagnostics = crate::json_array(&broken["doc_diagnostics"]["items"]);
    assert!(
        diagnostics
            .iter()
            .any(|item| item["kind"] == "broken_doc_link" && item["line"] == 8)
    );
    assert!(
        diagnostics
            .iter()
            .any(|item| item["kind"] == "broken_doc_link" && item["line"] == 9)
    );
    ws.write("src/main.rs", "// shifted\n// still a valid source range\npub fn target() {}\npub fn caller() { target(); }\npub fn grandcaller() { caller(); }\n");
    ws.delta(&["src/main.rs"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let stale = client
        .payload(
            "code_map_analyze",
            json!({"target":"docs/range.md","limit":100}),
        )
        .expect("MCP tool should succeed");
    let stale = crate::json_array(&stale["doc_diagnostics"]["items"])
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .collect::<Vec<_>>();
    assert_eq!(
        stale.len(),
        1,
        "generic, ambiguous, and out-of-bounds claims cannot prove stale source: {stale:?}"
    );
    assert_eq!(stale[0]["reason"], "documented_symbol_range_mismatch");
    assert_eq!(stale[0]["claim"]["target_path"], "src/main.rs");
    assert_eq!(stale[0]["claim"]["start_line"], 1);
    assert_eq!(stale[0]["claim"]["end_line"], 1);
    assert_eq!(stale[0]["target"]["path"], "src/main.rs");
    assert_eq!(stale[0]["target"]["line"], 3);
    assert_eq!(stale[0]["target"]["end_line"], 3);
    assert_eq!(stale[0]["line"], 2);
    ws.write(
        "docs/range.md",
        "# Range\n[`target`](../src/main.rs#L3)\n[`target`](../src/main.rs#L2-L4)\n",
    );
    ws.delta(&["docs/range.md"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let clear = client
        .payload(
            "code_map_analyze",
            json!({"target":"docs/range.md","limit":100}),
        )
        .expect("MCP tool should succeed");
    assert!(
        !crate::json_array(&clear["doc_diagnostics"]["items"])
            .iter()
            .any(|item| item["kind"] == "stale_doc_sections")
    );
    ws.write("src/main.rs", "pub fn target_renamed() {}\npub fn caller() { target_renamed(); }\npub fn grandcaller() { caller(); }\n");
    ws.delta(&["src/main.rs"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let dead = client
        .payload(
            "code_map_analyze",
            json!({"target":"docs/range.md","limit":100}),
        )
        .expect("MCP tool should succeed");
    let dead = crate::json_array(&dead["doc_diagnostics"]["items"])
        .iter()
        .find(|item| item["kind"] == "dead_doc_symbol")
        .unwrap();
    assert_eq!(dead["suggested_symbol"]["name"], "target_renamed");
    let document = client
        .document("docs/range.md", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert!(
        !crate::json_array(&document["referenced_symbols"]["items"])
            .iter()
            .any(|symbol| symbol["name"] == "target_renamed"),
        "a suggestion is not a verified documentary claim"
    );
    let affected = client
        .payload(
            "code_map_impact",
            json!({"selector":"src/main.rs","limit":100}),
        )
        .expect("MCP tool should succeed");
    assert!(
        crate::json_array(&affected["affected_docs"]["items"])
            .iter()
            .any(|section| section["path"] == "docs/range.md"),
        "deleting a documented symbol must retain affected documentation by its verified file claim: {affected}"
    );

    let relation_page0 = client
        .document("docs/guide.md", json!({"limit":1,"offset":0}))
        .expect("MCP tool should succeed");
    let relation_page1 = client
        .document(
            "docs/guide.md",
            json!({"limit":1,"offset":1,"generation":relation_page0["cites_doc"]["generation"]}),
        )
        .expect("MCP tool should succeed");
    assert!(
        !(relation_page0["cites_doc"]["total"] != 2
            || relation_page1["cites_doc"]["total"] != 2
            || relation_page0["cites_doc"]["items"][0]["path"]
                == relation_page1["cites_doc"]["items"][0]["path"]),
        "relation continuation must preserve total and return the second cited document: page0={relation_page0}, page1={relation_page1}"
    );

    let cli_first = ws
        .query(&["docs", "get", "docs/guide.md"])
        .expect("CLI should succeed");
    let mcp_first = client
        .document("docs/guide.md", json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(
        cli_first["cites_doc"]["total"],
        mcp_first["cites_doc"]["total"]
    );
    assert_eq!(
        cli_first["cites_doc"]["items"],
        mcp_first["cites_doc"]["items"]
    );
    client.finish().expect("server must finish successfully");
}
