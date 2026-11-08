use super::*;

#[test]
fn mcp_source_previews_respect_boundaries_and_continue_crlf_utf8() {
    let body = (0..90)
        .map(|i| format!("    value_{i} = 'Привіт'\r\n"))
        .collect::<String>();
    let source = format!(
        "def previous():\r\n    return 'secret'\r\n\r\n# selected\r\ndef selected():\r\n{body}    return value_0\r\ndef following(): return 'hidden'\r\n"
    );
    let full_body = (0..850)
        .map(|i| format!("    escaped_{i:04} = \"{}\"\r\n", r#"é\""#.repeat(4)))
        .collect::<String>();
    let full_source = format!("def full_symbol():\r\n{full_body}    return escaped_0000\r\n");
    let (workspace, harness) = McpTestHarness::with_files(&[
        (
            "forge-mcp.yaml",
            "roots: [.]\nresponse:\n  max_output_bytes: 65536\n  source_context:\n    max_body_lines: 24\n",
        ),
        ("service.py", source.as_str()),
        ("full.py", full_source.as_str()),
    ]);
    let mut client = harness.expect("MCP initialize");
    let default = client
        .inspect("selected", json!({}))
        .expect("MCP tool should succeed");
    assert!(default.get("source").is_none());
    let explanation = client
        .explain("selected", json!({"show_source":true}))
        .expect("MCP tool should succeed");
    assert!(explanation["source"]
        .as_str()
        .unwrap()
        .contains("def selected"));
    assert_eq!(explanation["source_preview"]["body_lines"], 24);
    assert!(explanation.get("incoming").is_some());
    assert_eq!(explanation["relations"], json!(["incoming"]));

    let inspection = client
        .inspect("full.py:full_symbol", json!({"show_source":true}))
        .expect("MCP tool should succeed");
    assert_eq!(inspection["source_preview"]["body_lines"], 24);
    assert_eq!(
        inspection["source_preview"]["total_body_lines"],
        full_source.lines().count()
    );
    assert_eq!(inspection["source_preview"]["has_more"], true);

    let cli = workspace
        .query(&["query", "inspect", "full.py:full_symbol", "--show-source"])
        .expect("CLI should succeed");
    assert!(cli["source"]
        .as_str()
        .unwrap()
        .starts_with("def full_symbol():"));
    assert!(cli["source_preview"]["body_lines"].as_u64().unwrap() > 0);

    for (label, max_body_lines) in [
        ("default", None),
        ("explicit-full", Some(full_source.lines().count())),
    ] {
        let mut arguments = json!({
            "selector":"full.py:full_symbol",
            "leading_lines":0,
            "format":"json"
        });
        if let Some(lines) = max_body_lines {
            arguments["max_body_lines"] = json!(lines);
        }
        let (full_bytes, full_response) =
            client.call("forge_read", arguments).expect("MCP transport");
        assert_ne!(
            full_response["result"]["isError"], true,
            "{label}: {full_response}"
        );
        assert!(
            full_bytes <= MAX_OUTPUT_BYTES,
            "{label} frame was {full_bytes} bytes"
        );
        let full = assertions::mcp_payload(&full_response);
        assert!(
            full["source"].as_str() == Some(full_source.as_str()),
            "{label} snippet was {} bytes, expected {} bytes; preview={}",
            full["source"].as_str().map(str::len).unwrap_or_default(),
            full_source.len(),
            full["source_preview"]
        );
        assert_eq!(
            full["source_preview"]["body_lines"],
            full_source.lines().count()
        );
        assert_eq!(full["source_preview"]["has_more"], false);
        assert_eq!(full["source_preview"]["next_source_offset"], Value::Null);
        assert_eq!(full["source_preview"]["truncated_line"], false);
        if label == "explicit-full" {
            assert!(
                full_bytes > 65_536 * 3 / 4,
                "frame was only {full_bytes} bytes"
            );
        }
    }

    let smaller = client
        .source(
            "full.py:full_symbol",
            json!({"leading_lines":0,"max_body_lines":17}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(smaller["source_preview"]["body_lines"], 17);
    assert_eq!(smaller["source_preview"]["next_source_offset"], 17);
    assert_eq!(smaller["source_preview"]["has_more"], true);
    let first = client
        .source("selected", json!({"max_body_lines":35}))
        .expect("MCP tool should succeed");
    let text = first["source"].as_str().unwrap();
    assert!(text.contains("Привіт\r\n") || text.contains("Привіт'\r\n"));
    assert!(!text.contains("previous"));
    assert!(!text.contains("following"));
    assert_eq!(first["source_preview"]["body_lines"], 35);
    let (_, wrong_cursor) = client
        .call(
            "forge_read",
            json!({
                "selector":"selected","max_body_lines":35,"source_offset":36,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_eq!(wrong_cursor["result"]["isError"], true);
    assert!(wrong_cursor["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("generation"));
    let second = client.source("selected", json!({"max_body_lines":35,"source_offset":first["source_preview"]["next_source_offset"],"generation":first["generation"]})).expect("MCP tool should succeed");
    assert_eq!(second["source_preview"]["body_offset"], 35);
    assert!(!second["source"].as_str().unwrap().contains("def selected"));
    assert_eq!(second["generation"], first["generation"]);
    let third = client.source("selected", json!({"max_body_lines":35,"source_offset":second["source_preview"]["next_source_offset"],"generation":second["generation"]})).expect("MCP tool should succeed");
    assert_eq!(third["source_preview"]["body_offset"], 70);
    let (_, markdown_first) = client
        .call(
            "forge_read",
            json!({
                "selector":"selected","max_body_lines":5,"leading_lines":0
            }),
        )
        .expect("MCP transport");
    assert_ne!(
        markdown_first["result"]["isError"], true,
        "{markdown_first}"
    );
    let (_, markdown_next) = client.call("forge_read", json!({
        "selector":"selected","max_body_lines":5,"leading_lines":0,"source_offset":5,"generation":first["generation"]
    })).expect("MCP transport");
    assert_ne!(
        markdown_next["result"]["isError"], true,
        "Markdown continuation with explicit generation failed: {markdown_next}"
    );
    assert!(markdown_next["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("**next_source_offset**: `10`"));

    for (tool, args) in [
        (
            "code_map_inspect",
            json!({"selector":"selected","show_source":true,"max_body_lines":12,"format":"json"}),
        ),
        (
            "code_map_explain",
            json!({"selector":"selected","show_source":true,"max_body_lines":12,"format":"json"}),
        ),
    ] {
        let initial = client
            .payload(tool, args.clone())
            .expect("MCP tool should succeed");
        assert_eq!(
            initial["source_preview"]["next_source_offset"], 12,
            "{tool}"
        );
        let mut continuation = args;
        continuation["source_offset"] = json!(12);
        let (_, missing) = client
            .call(tool, continuation.clone())
            .expect("MCP transport");
        assert_eq!(missing["result"]["isError"], true, "{tool}: {missing}");
        continuation["generation"] = initial["generation"].clone();
        let next = client
            .payload(tool, continuation)
            .expect("MCP tool should succeed");
        assert_eq!(next["source_preview"]["body_offset"], 12, "{tool}");
        assert_eq!(next["generation"], initial["generation"], "{tool}");
    }

    let mut restarted = McpTestHarness::new(&workspace).expect("MCP initialize");
    let (_, restarted_response) = restarted
        .call(
            "forge_read",
            json!({
                "selector":"selected","max_body_lines":35,"source_offset":35,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_eq!(restarted_response["result"]["isError"], true);
    assert!(restarted_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("previous page's generation"));
    let (_, foreign_symbol) = client
        .call(
            "forge_read",
            json!({
                "selector":"selected_116","max_body_lines":35,"source_offset":35,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_eq!(foreign_symbol["result"]["isError"], true);
    let (_, unrelated_page) = client.call(
        "forge_read",
        json!({
            "selector":"selected","max_body_lines":35,"offset":1,"source_offset":35,"format":"json"
        }),
    ).expect("MCP transport");
    assert_eq!(
        unrelated_page["result"]["isError"], true,
        "source token cannot satisfy collection pagination: {unrelated_page}"
    );
    assert!(unrelated_page["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("continuation"));
    let (_, disabled_page) = client.call("code_map_inspect", json!({"selector":"full.py:full_symbol","show_source":false,"max_body_lines":12,"format":"json"})).expect("MCP transport");
    assert_ne!(disabled_page["result"]["isError"], true, "{disabled_page}");
    let (_, disabled_continuation) = client.call("code_map_inspect", json!({
        "selector":"full.py:full_symbol","show_source":true,"max_body_lines":12,"source_offset":12,"format":"json"
    })).expect("MCP transport");
    assert_eq!(disabled_continuation["result"]["isError"], true);
    let first_generation = first["generation"].as_str().unwrap().to_owned();
    let changed_source = source.replace("value_0 = 'Привіт'", "value_0 = 'Змінено'");
    workspace.write("service.py", &changed_source);
    workspace.delta(&["service.py"]);
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let second_generation = client
        .source(
            "selected",
            json!({"max_body_lines":35,"format":"json"
            }),
        )
        .expect("MCP tool should succeed");
    assert_ne!(second_generation["generation"], first["generation"]);
    let (_, ambiguous) = client
        .call(
            "forge_read",
            json!({
                "selector":"selected","max_body_lines":35,"source_offset":35,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_eq!(ambiguous["result"]["isError"], true);
    assert!(ambiguous["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("generation"));
    let (_, stale_explicit) = client.call("forge_read", json!({
        "selector":"selected","max_body_lines":35,"source_offset":35,"generation":first_generation,"format":"json"
    })).expect("MCP transport");
    assert_eq!(
        stale_explicit["result"]["isError"], true,
        "explicit stale token remains authoritative: {stale_explicit}"
    );
    let current = client.source("selected", json!({"max_body_lines":35,"source_offset":35,"generation":second_generation["generation"],"format":"json"
    })).expect("MCP tool should succeed");
    assert_eq!(current["source_preview"]["body_offset"], 35);

    let selector = "full.py:full_symbol";
    let max_body_lines = full_source.lines().count();
    let (_, complete_page) = client
        .call(
            "forge_read",
            json!({
                "selector":selector,"max_body_lines":max_body_lines,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_ne!(complete_page["result"]["isError"], true, "{complete_page}");
    let (_, missing_token) = client.call(
        "forge_read",
        json!({
            "selector":selector,"max_body_lines":max_body_lines,"source_offset":1,"format":"json"
        }),
    ).expect("MCP transport");
    assert_eq!(missing_token["result"]["isError"], true);
    workspace.write(
        "huge.py",
        &format!("def enormous(): return '{}'\n", "Привіт".repeat(50_000)),
    );
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let (_, enormous) = client
        .call("forge_read", json!({"selector":"enormous"}))
        .expect("MCP transport");
    assert_eq!(enormous["result"]["isError"], true);
    assert!(enormous["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("huge.py"));
    let (_, enormous_missing_token) = client
        .call(
            "forge_read",
            json!({
                "selector":"enormous","source_offset":1,"format":"json"
            }),
        )
        .expect("MCP transport");
    assert_eq!(enormous_missing_token["result"]["isError"], true);

    client.finish().expect("server must finish successfully");
    restarted.finish().expect("server must finish successfully");

    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  max_output_bytes: 4096\n  source_context:\n    max_body_lines: 16\n",
    );
    let bounded_body = (0..700)
        .map(|i| format!("    value_{i:04} = 'x{}\\\\\"Привіт'\n", "x".repeat(36)))
        .collect::<String>();
    let bounded_source = format!("def bounded():\n{bounded_body}    return value_0000\n");
    workspace.write("bounded.py", &bounded_source);
    workspace.delta(&["bounded.py"]);
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let mut bounded_client = McpTestHarness::new(&workspace).expect("MCP initialize");
    let mut offset = 0;
    let mut generation = Value::Null;
    let mut reconstructed = String::new();
    loop {
        let mut arguments = json!({
            "selector":"bounded.py:bounded",
            "leading_lines":0,
            "max_body_lines":800,
            "format":"json"
        });
        if offset > 0 {
            arguments["source_offset"] = json!(offset);
            arguments["generation"] = generation.clone();
        }
        let (frame_bytes, response) = bounded_client
            .call("forge_read", arguments)
            .expect("MCP transport");
        assert!(
            frame_bytes <= 4096,
            "configured response was {frame_bytes} bytes"
        );
        assert_ne!(response["result"]["isError"], true, "{response}");
        let page = assertions::mcp_payload(&response);
        let preview = &page["source_preview"];
        assert_eq!(preview["body_offset"], offset);
        assert_eq!(preview["total_body_lines"], bounded_source.lines().count());
        reconstructed.push_str(page["source"].as_str().unwrap());
        let next = preview["next_source_offset"].as_u64();
        if let Some(next) = next {
            assert_eq!(next, offset + preview["body_lines"].as_u64().unwrap());
            assert!(next > offset);
            assert_eq!(preview["has_more"], true);
            assert!(preview["omitted_body_lines"].as_u64().unwrap() > 0);
            generation = page["generation"].clone();
            offset = next;
        } else {
            assert_eq!(preview["has_more"], false);
            assert_eq!(preview["omitted_body_lines"], 0);
            break;
        }
    }
    assert_eq!(reconstructed, bounded_source);
    let (markdown_bytes, markdown_response) = bounded_client
        .call(
            "forge_read",
            json!({
                "selector":"bounded.py:bounded",
                "leading_lines":0,
                "max_body_lines":800,
                "format":"markdown"
            }),
        )
        .expect("MCP transport");
    assert!(
        markdown_bytes <= 4096,
        "Markdown response was {markdown_bytes} bytes"
    );
    assert_ne!(
        markdown_response["result"]["isError"], true,
        "{markdown_response}"
    );
    let markdown_text = markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(markdown_text.contains("value_0000"));
    bounded_client
        .finish()
        .expect("server must finish successfully");
}

#[cfg(feature = "lang-python")]
#[test]
fn repair_ast_horizon_has_no_unusable_cursor() {
    let source = (0..10_001)
        .map(|i| format!("print({i})\n"))
        .collect::<String>();
    let (_workspace, harness) = McpTestHarness::with_files(&[("service.py", source.as_str())]);
    let mut client = harness.expect("MCP initialize");
    let first = client
        .payload(
            "ast_grep_search",
            json!({"pattern":"print($VALUE)","language":"python","limit":100}),
        )
        .expect("MCP tool should succeed");
    let last = client.payload("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","offset":9900,"limit":100,"generation":first["matches"]["generation"]})).expect("MCP tool should succeed");
    let page = &last["matches"];
    assert_eq!(crate::json_array(&page["items"]).len(), 100);
    assert_eq!(page["has_more"], true);
    assert_eq!(page["computation_truncated"], true);
    assert!(
        page["next_offset"].is_null(),
        "unusable cursor: {}",
        page["next_offset"]
    );
    assert!(page["total"].is_null());
    assert!(page["continuation_hint"]
        .as_str()
        .unwrap()
        .contains("narrow"));
    client.finish().expect("server must finish successfully");
}

#[cfg(feature = "lang-python")]
#[test]
fn repair_ast_byte_pruning_keeps_earlier_horizon_continuation_usable() {
    let source = (0..1_001)
        .map(|i| format!("print({i})\n"))
        .collect::<String>();
    let (_workspace, harness) = McpTestHarness::with_files(&[
        ("forge-mcp.yaml", "response:\n  max_output_bytes: 4096\n"),
        ("service.py", source.as_str()),
    ]);
    let mut client = harness.expect("MCP initialize");
    let first = client
        .payload(
            "ast_grep_search",
            json!({"pattern":"print($VALUE)","language":"python","limit":100}),
        )
        .expect("MCP tool should succeed");
    let mut offset = 900;
    let mut lines = Vec::new();
    loop {
        let payload = client.payload("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","offset":offset,"limit":100,"generation":first["matches"]["generation"]})).expect("MCP tool should succeed");
        let page = &payload["matches"];
        let items = crate::json_array(&page["items"]);
        assert!(!items.is_empty());
        lines.extend(items.iter().map(|item| item["line"].as_u64().unwrap()));
        if page["next_offset"].is_null() {
            assert_eq!(page["has_more"], false);
            assert_eq!(lines.last(), Some(&1_001));
            break;
        }
        let next = page["next_offset"].as_u64().unwrap();
        assert_eq!(next, offset + items.len() as u64);
        assert!(next > offset && next < 1_001);
        offset = next;
    }
    assert_eq!(lines, (901..=1_001).collect::<Vec<_>>());
    client.finish().expect("server must finish successfully");
}

#[cfg(feature = "lang-python")]
#[test]
fn repair_variadic_matcher_budget_is_checked_inside_backtracking() {
    let source = format!("f({})\nmissing = 0\n", vec!["0"; 400].join(","));
    let (workspace, harness) = McpTestHarness::with_files(&[("service.py", source.as_str())]);
    let mut client = harness.expect("MCP initialize");
    let started = std::time::Instant::now();
    let payload = client
        .payload(
            "ast_grep_search",
            json!({"pattern":"f($$$A, $$$B, $$$C, missing)","language":"python","limit":1}),
        )
        .expect("MCP tool should succeed");
    let elapsed = started.elapsed();
    eprintln!("MCP-02: variadic missing-terminal MCP response completed in {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(4),
        "recursive matcher exceeded cooperative2s budget plus2s margin: {elapsed:?}"
    );
    assert_eq!(payload["matches"]["computation_truncated"], true);
    assert!(payload["matches"]["total"].is_null());
    assert!(payload["matches"]["next_offset"].is_null());
    assert_eq!(payload["matches"]["has_more"], true);
    assert!(payload["matches"]["generation"].is_string());
    assert!(payload["matches"]["continuation_hint"]
        .as_str()
        .unwrap()
        .contains("narrow path or pattern"));
    let source = "f(1, 2, missing)\nf(1, 1)\nf(1, 2)\n";
    workspace.write("service.py", source);
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    for pattern in ["f($$$A, $$$B, missing)", "f($VALUE, $VALUE)"] {
        let ordinary = client
            .payload(
                "ast_grep_search",
                json!({"pattern":pattern,"language":"python","limit":100}),
            )
            .expect("MCP tool should succeed");
        let legacy = contextunity_forge_mcp::engine::ast::search(
            source,
            "service.py",
            "python",
            pattern,
            100,
        )
        .unwrap();
        assert!(!legacy.is_empty());
        assert_eq!(ordinary["matches"]["items"], json!(legacy));
        assert_eq!(ordinary["matches"]["computation_truncated"], false);
    }
    let late_source = (0..25_001)
        .map(|i| format!("print({i})\n"))
        .collect::<String>();
    let legacy = contextunity_forge_mcp::engine::ast::search(
        &late_source,
        "late.py",
        "python",
        "print(25000)",
        1,
    )
    .unwrap();
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0]["line"], 25_001);
    client.finish().expect("server must finish successfully");
}
