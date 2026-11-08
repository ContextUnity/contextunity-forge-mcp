use super::*;

#[test]
fn mcp_budget_and_query_errors_explain_how_to_retry() {
    let mut client =
        McpTestHarness::indexed(&[("README.md", "# Indexed document\n")]).expect("MCP initialize");
    for (name, arguments, expected) in [
        (
            "code_map_analyze",
            json!({"target":"SELECT hex(zeroblob(3000000)) AS a, hex(zeroblob(3000000)) AS b"}),
            "select fewer and smaller SQL columns",
        ),
        (
            "code_map_analyze",
            json!({"target":"diagnostics"}),
            "Use target='' for workspace diagnostics",
        ),
        (
            "code_map_query",
            json!({"operation":"cypher","selector":"MATCH (n) RETURN n"}),
            "supported: overview,inspect,explain,impact,slice,unwired,sql,untested,implementations",
        ),
        (
            "code_map_inspect",
            json!({"selector":"file:nonexistent.rs"}),
            "File paths are passed without a file: prefix",
        ),
    ] {
        let (_, response) = client.call(name, arguments).expect("MCP transport");
        assert_eq!(response["result"]["isError"], true, "{response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains(expected), "{name}: {message}");
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_analyze_preserves_sql_columns_named_like_metadata() {
    let mut client =
        McpTestHarness::indexed(&[("main.py", "def main(): pass\n")]).expect("MCP initialize");
    let details = json!({
        "nodes": {"items": [{"output_root": "node-root-12345", "corpus_hash": "node-hash-123456"}]},
        "symbols": [{"output_root": "symbol-root-123", "corpus_hash": "symbol-hash-1234"}],
        "doc_sections": [{"output_root": "section-root-12", "corpus_hash": "section-hash-123"}],
        "freshness": {"output_root": "nested-root-1234", "corpus_hash": "nested-hash-12345"},
        "metadata": {"output_root": "nested-meta-1234", "corpus_hash": "nested-meta-12345"}
    })
    .to_string()
    .replace('\'', "''");
    let result = client.payload(
        "code_map_analyze",
        json!({"format":"json","target":format!("SELECT '1234567é' AS generation, '0123456789' AS output_root, 'abcdef0123456789' AS corpus_hash, '{details}' AS details")}),
    ).expect("MCP tool should succeed");
    assert_eq!(result["rows"]["items"][0]["generation"], "1234567é");
    assert_eq!(result["rows"]["items"][0]["output_root"], "0123456789");
    assert_eq!(
        result["rows"]["items"][0]["corpus_hash"],
        "abcdef0123456789"
    );
    let returned_details = result["rows"]["items"][0]["details"].as_str().unwrap();
    assert_eq!(returned_details, details.as_str());

    // Real SQL values are opaque even when they resemble operation metadata.
    let sql = client.payload("code_map_analyze", json!({"format":"json",
        "target":"SELECT 'sql-generation-must-remain-complete' AS generation, 0 AS count, json('false') AS complete, json('[]') AS items"})).expect("MCP tool should succeed");
    assert_eq!(
        sql["rows"]["items"][0]["generation"],
        "sql-generation-must-remain-complete"
    );
    assert_eq!(sql["rows"]["items"][0]["count"], 0);
    assert_eq!(sql["rows"]["items"][0]["items"], "[]");

    assert_eq!(sql["rows"]["items"][0]["complete"], "false");
    let coordinate_named_columns = client.payload(
        "code_map_analyze",
        json!({
            "format":"json",
            "target":"SELECT 'opaque path # λ' AS path, 37 AS line, 'file:///literal%20uri#L9' AS uri"
        }),
    ).expect("MCP tool should succeed");
    assert_eq!(
        coordinate_named_columns["rows"]["items"][0],
        json!({
            "path":"opaque path # λ",
            "line":37,
            "uri":"file:///literal%20uri#L9"
        })
    );
    let complete = json!({"total":1,"offset":0,"limit":1,"has_more":false,
        "next_offset":null,"generation":"inactive-display-generation","items":[{"name":"retained"}]});
    let durable = json!({"task_id":"project/milestone:task","stage":"build","claim_revision":0,
        "contract_revision":2,"worker_id":"worker","worktree":"/workspace",
        "candidate_hash":"candidate-hash-must-remain-complete",
        "content_hash":"content-hash-must-remain-complete","receipt":{"generation":"durable-generation"},
        "workflow_guidance":{"rules":["ISOLATION: assigned worktree"],
            "allowed_write_scope":["src/core/format/"],"subtask_dod":["production seam"],
            "expected_evidence_template":{"proof":{"build_proof":{"exit_code":0}}},
            "commands":{"test":"cargo test --test mcp"}},"complete":false});
    for (case, mut semantic) in [
        (
            "inactive",
            json!({"nodes":complete,"generation":"inactive-display-generation",
            "count":0,"complete":false,"freshness":{"status":"matched","output_root":"display-root",
                "corpus_hash":"display-corpus","refresh":"none"}}),
        ),
        (
            "nested",
            json!({"incoming":complete,"outgoing":{"total":2,"offset":0,"limit":1,
            "items":[{"name":"retained"}],"has_more":true,"next_offset":1,
            "generation":"independent-nested-continuation-token"}}),
        ),
        (
            "source",
            json!({"nodes":complete,"source":"def selected():",
            "source_preview":{"next_source_offset":3,"has_more":true},
            "generation":"source-only-continuation-token"}),
        ),
        ("durable", durable.clone()),
    ] {
        response::compact_mcp_metadata(&mut semantic);
        match case {
            "inactive" => {
                assert!(semantic.get("generation").is_none(), "{semantic}");
                assert!(semantic["nodes"].get("generation").is_none(), "{semantic}");
                assert!(
                    semantic["freshness"].get("generation").is_none(),
                    "{semantic}"
                );
                assert!(
                    semantic["freshness"].get("output_root").is_none(),
                    "{semantic}"
                );
                assert_eq!(semantic["count"], 0);
                assert_eq!(semantic["complete"], false);
            }
            "nested" => {
                assert_eq!(
                    semantic["outgoing"]["generation"],
                    "independent-nested-continuation-token"
                );
                assert_eq!(semantic["outgoing"]["next_offset"], 1);
            }
            "source" => assert_eq!(semantic["generation"], "source-only-continuation-token"),
            "durable" => {
                assert_eq!(semantic, durable);
                let rendered = response::result(Ok(semantic), &ResponsePolicy::default());
                assert_ne!(rendered.is_error, Some(true));
                let text = rendered.content[0].as_text().unwrap().text.as_str();
                for preserved in [
                    "candidate-hash-must-remain-complete",
                    "content-hash-must-remain-complete",
                    "durable-generation",
                    "project/milestone:task",
                    "ISOLATION: assigned worktree",
                    "src/core/format/",
                    "production seam",
                    "cargo test --test mcp",
                    "expected_evidence_template",
                    "claim_revision",
                    "contract_revision",
                ] {
                    assert!(
                        text.contains(preserved),
                        "durable guidance lost {preserved}: {text}"
                    );
                }
            }
            _ => unreachable!(),
        }
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_analyze_rejects_oversized_intermediate_sql_values() {
    let mut client =
        McpTestHarness::indexed(&[("main.py", "def main(): pass\n")]).expect("MCP initialize");
    let exact = client
        .payload(
            "code_map_analyze",
            json!({"target":"SELECT length(randomblob(8388608)) AS payload"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(exact["rows"]["items"][0]["payload"], 8388608);
    let (_, response) = client
        .call(
            "code_map_analyze",
            json!({"target":"SELECT length(randomblob(8388609)) AS payload"}),
        )
        .expect("MCP transport");
    assert_eq!(response["result"]["isError"], true, "{response}");
    client.finish().expect("server must finish successfully");
}

#[test]
fn cold_build_populates_document_fts_search() {
    let (_workspace, harness) = McpTestHarness::with_files(&[(
        "guide.md",
        "# Guide\nThis document contains quasarneedle for search.\n",
    )]);
    let mut client = harness.expect("MCP initialize");
    let result = client.payload(
        "code_map_analyze",
        json!({"target":"SELECT count(*) AS matches FROM doc_search WHERE doc_search MATCH 'quasarneedle'"}),
    ).expect("MCP tool should succeed");
    assert_eq!(result["rows"]["items"][0]["matches"], 1);
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_caps_documents_scalars_checkpoints_and_tool_errors() {
    let readme = format!("# Huge document\n{}", "\"\\Привіт\t".repeat(30_000));
    let (_workspace, harness) = McpTestHarness::with_files(&[
        (
            "forge-mcp.yaml",
            "roots: [.]\ntasks_db: .forge/tasks.sqlite\nresponse:\n  max_output_bytes: 1024\n",
        ),
        ("README.md", readme.as_str()),
    ]);
    let mut client = harness.expect("MCP initialize");
    let (_, tools) = client
        .request("tools/list", json!({}))
        .expect("MCP transport");
    assert!(crate::json_array(&tools["result"]["tools"]).len() >= 15);
    let cases = [
        ("get_doc", json!({"path_or_id":"README.md","detail":"full"})),
        (
            "code_map_analyze",
            json!({"target":"SELECT replace(hex(zeroblob(200000)), '0', char(34)) AS giant"}),
        ),
        (
            "code_map_query",
            json!({"operation":"\"\\Привіт".repeat(30_000)}),
        ),
        (
            "code_map_search",
            json!({"pattern":"*","detail":"\"\\Привіт".repeat(30_000)}),
        ),
    ];
    for (name, arguments) in cases {
        let (_, response) = client.call(name, arguments).expect("MCP transport");
        assert!(
            response["result"]["isError"] == true || response.get("error").is_some(),
            "{name}: {response}"
        );
    }
    let invalid_formats = [
        json!("\0".repeat(2_000)),
        json!(true),
        json!({"nested":["\"\\\nПривіт".repeat(2_000)]}),
    ];
    for (index, invalid_format) in invalid_formats.into_iter().enumerate() {
        let name = format!("forge-mcp/forge-mcp/m-test/tasks/invalid-format-{index}/build");
        let (bytes, response) = client
            .call(
                "session_checkpoint",
                json!({
                    "format":invalid_format,
                    "action":"save",
                    "name":name.clone(),
                    "content":{"must_not_persist":true}
                }),
            )
            .expect("MCP transport");
        assert!(
            bytes <= 1024,
            "invalid format response frame was {bytes} bytes"
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        let error: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(error["error"]["code"], "INVALID_FORMAT", "{error}");
        assert!(
            client
                .payload("session_checkpoint", json!({"action":"list"}))
                .expect("checkpoint list should succeed")
                .get(&name)
                .is_none(),
            "invalid format persisted checkpoint {name}"
        );
    }
    let (_, unknown) = client
        .call(&"\"\\Привіт".repeat(30_000), json!({}))
        .expect("MCP transport");
    assert!(unknown["result"]["isError"] == true || unknown.get("error").is_some());
    let large_checkpoint = "forge-mcp/forge-mcp/m-test/tasks/large/build";
    client
        .payload(
            "session_checkpoint",
            json!({"action":"save","name":large_checkpoint,"content":"\"\\Привіт".repeat(20_000)}),
        )
        .expect("MCP tool should succeed");
    let (_, response) = client
        .call(
            "session_checkpoint",
            json!({"action":"get","name":large_checkpoint}),
        )
        .expect("MCP transport");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        client
            .payload("session_checkpoint", json!({"action":"list"}))
            .expect("MCP tool should succeed"),
        json!({(large_checkpoint):{"bytes":320002}})
    );
    let (_, response) = client
        .call("get_doc", json!({"path_or_id":"README.md","limit":101}))
        .expect("MCP transport");
    assert_eq!(response["result"]["isError"], true);
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_byte_pruning_continues_after_exactly_the_emitted_rows() {
    let (_workspace, harness) = McpTestHarness::with_files(&[("README.md", "# Small\nindexed\n")]);
    let mut client = harness.expect("MCP initialize");
    let sql = "WITH RECURSIVE t(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM t WHERE n<16) SELECT n,replace(hex(zeroblob(1800)), '0', char(34)) AS escaped FROM t ORDER BY n";
    let mut offset = 0;
    let mut generation = Value::Null;
    let mut seen = Vec::new();
    loop {
        let payload = client
            .payload(
                "code_map_analyze",
                json!({"target":sql,"limit":30,"offset":offset,"generation":generation}),
            )
            .expect("MCP tool should succeed");
        let page = &payload["rows"];
        assert_eq!(page["total"], 17);
        let items = crate::json_array(&page["items"]);
        assert!(!items.is_empty(), "zero progress page: {page}");
        seen.extend(items.iter().map(|item| item["n"].as_u64().unwrap()));
        if page["has_more"] == false {
            break;
        }
        assert_eq!(
            page["next_offset"].as_u64().unwrap(),
            offset + items.len() as u64
        );
        offset = page["next_offset"].as_u64().unwrap();
        generation = page["generation"].clone();
    }
    assert_eq!(seen, (0..17).collect::<Vec<_>>());

    // This real response boundary must decide token liveness after byte trimming.
    let policy = ResponsePolicy {
        max_output_bytes: 2048,
        ..ResponsePolicy::default()
    };
    let bounded = response::result(
        Ok(json!({"nodes":{"total":3,"offset":0,"limit":3,
        "has_more":false,"next_offset":null,"generation":"tail-trim-continuation-token",
        "items":[{"name":"first-retained","text":"x".repeat(800)},
            {"name":"middle","text":"x".repeat(800)},
            {"name":"last-omitted","text":"x".repeat(800)}]}})),
        &policy,
    );
    assert_ne!(bounded.is_error, Some(true), "{bounded:?}");
    assert!(response::serialized_bytes(&bounded) <= policy.max_output_bytes - 256);
    let text = bounded.content[0].as_text().unwrap().text.as_str();
    for retained in ["first-retained", "tail-trim-continuation-token", "offset=1"] {
        assert!(
            text.contains(retained),
            "tail trimming lost {retained}: {text}"
        );
    }
    assert!(
        !text.contains("last-omitted"),
        "fixture did not exercise tail trimming: {text}"
    );
    let oversized = response::result(Ok(json!({"scalar":"x".repeat(5000)})), &policy);
    assert_eq!(oversized.is_error, Some(true));
    assert!(response::serialized_bytes(&oversized) <= policy.max_output_bytes - 256);
    client.finish().expect("server must finish successfully");
}

#[test]
fn adapter_response_policy_reloads_without_reindexing_and_rejects_invalid_settings() {
    let (workspace, harness) = McpTestHarness::with_files(&[
        (
            "forge-mcp.yaml",
            "roots: [.]\nresponse:\n  page_size: 2\n  max_output_bytes: 4096\n",
        ),
        ("README.md", "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n"),
    ]);

    let first = scanner::load_adapter(workspace.root(), None).unwrap();
    let mut client = harness.expect("MCP initialize");
    let payload = client
        .document("README.md", json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(crate::json_array(&payload["sections"]["items"]).len(), 2);
    let database = workspace.db();
    let before = fs::metadata(&database).unwrap().modified().unwrap();
    workspace.write("forge-mcp.yaml", "roots: [.]\nresponse:\n  detail: full\n  page_size: 3\n  max_output_bytes: 2048\n  source_context:\n    max_body_lines: 10\n");
    let second = scanner::load_adapter(workspace.root(), None).unwrap();
    assert_eq!(first.digest, second.digest);
    let (bytes, response) = client
        .call("get_doc", json!({"path_or_id":"README.md","format":"json"}))
        .expect("MCP transport");
    assert!(bytes <= 2048, "{bytes}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(fs::metadata(database).unwrap().modified().unwrap(), before);
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  max_output_bytes: 1024\n",
    );
    let (bytes, response) = client
        .call(
            "code_map_search",
            json!({"pattern":"*","detail":"A".repeat(10_000)}),
        )
        .expect("MCP transport");
    assert!(bytes <= 1024, "parameter error frame was {bytes} bytes");
    assert_eq!(response["result"]["isError"], true);
    client.finish().expect("server must finish successfully");
    workspace.write("forge-mcp.yaml", "response:\n  max_output_bytes: 98304\n");
    assert!(scanner::load_adapter(workspace.root(), None).is_ok());
    for settings in [
        "page_size: 0",
        "page_size: 101",
        "max_output_bytes: 98305",
        "max_output_bytes: 1023",
        "source_context: {leading_lines: 21}",
        "source_context: {max_body_lines: 501}",
        "source_context: {max_body_lines: 0}",
        "detail: enormous",
    ] {
        workspace.write("forge-mcp.yaml", &format!("response:\n  {settings}\n"));
        assert!(
            scanner::load_adapter(workspace.root(), None).is_err(),
            "{settings}"
        );
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn mcp_default_pages_and_stale_continuation_are_explicit() {
    let source = (0..125)
        .map(|i| format!("def symbol_{i:03}():\n    return {i}\n"))
        .collect::<String>();
    let (workspace, harness) =
        McpTestHarness::indexed_workspace(&[("service.py", source.as_str())]);
    let mut client = harness.expect("MCP initialize");
    let initial = client
        .search("symbol_*", json!({"kind":"function"}))
        .expect("MCP tool should succeed");
    let generation = initial["nodes"]["generation"].as_str().unwrap();
    assert!(!generation.is_empty());
    assert_eq!(initial["nodes"]["limit"], 30);
    assert_eq!(crate::json_array(&initial["nodes"]["items"]).len(), 30);
    assert_eq!(initial["nodes"]["total"], 125);
    let next = client.search("symbol_*", json!({"kind":"function","limit":100,"offset":30,"generation":initial["nodes"]["generation"]})).expect("MCP tool should succeed");
    assert_eq!(crate::json_array(&next["nodes"]["items"]).len(), 95);
    assert_eq!(next["nodes"]["has_more"], false);
    let mut colliding_prefix = generation.to_owned();
    colliding_prefix.replace_range(0..1, if &generation[0..1] == "0" { "1" } else { "0" });
    let (_, stale_prefix) = client
        .call(
            "code_map_search",
            json!({"pattern":"symbol_*","limit":100,"offset":30,"generation":colliding_prefix}),
        )
        .expect("MCP transport");
    assert_eq!(stale_prefix["result"]["isError"], true);
    let (_, missing) = client
        .call("code_map_search", json!({"pattern":"symbol_*","offset":30}))
        .expect("MCP transport");
    assert_eq!(missing["result"]["isError"], true);
    workspace.write("service.py", &format!("{source}\ndef added(): return 0\n"));
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let (_, stale) = client
        .call(
            "code_map_search",
            json!({"pattern":"symbol_*","offset":30,"generation":initial["nodes"]["generation"]}),
        )
        .expect("MCP transport");
    assert_eq!(stale["result"]["isError"], true);
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_tool_catalog_allows_optional_json_and_markdown_formats() {
    let mut client = McpTestHarness::indexed(&[]).expect("MCP initialize");
    let listed = client
        .request_value("tools/list", json!({}))
        .expect("MCP transport");
    for tool in crate::json_array(&listed["result"]["tools"]) {
        let format = &tool["inputSchema"]["properties"]["format"];
        assert_eq!(format["type"], "string", "tool {}: {format}", tool["name"]);
        assert_eq!(
            format["enum"],
            json!(["markdown", "json"]),
            "tool {} has an unexpected format selector: {format}",
            tool["name"]
        );
        let format_is_required =
            tool["inputSchema"]["required"]
                .as_array()
                .is_some_and(|required| {
                    required
                        .iter()
                        .any(|field| field.as_str() == Some("format"))
                });
        assert!(
            !format_is_required,
            "tool {} must permit omitted/default Markdown format",
            tool["name"]
        );
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn task_submit_format_preserves_optional_input_and_evidence_semantics() {
    use contextunity_forge_mcp::mcp::tools::TaskSubmitInput;

    for (format, expected) in [
        (None, None),
        (Some(json!("markdown")), Some(OutputFormat::Markdown)),
        (Some(json!("json")), Some(OutputFormat::Json)),
    ] {
        for evidence in [None, Some(json!({"proof": "caller supplied"}))] {
            let mut input = json!({"task_id":"format-contract", "stage":"build", "action":"pass"});
            if let Some(format) = &format {
                input["format"] = format.clone();
            }
            if let Some(evidence) = &evidence {
                input["evidence"] = evidence.clone();
            }
            let parsed: TaskSubmitInput = serde_json::from_value(input).unwrap();
            assert_eq!(parsed.format, expected);
            assert_eq!(
                parsed.format.unwrap_or_default(),
                expected.unwrap_or(OutputFormat::Markdown)
            );
            assert_eq!(parsed.evidence_provided, evidence.is_some());
            assert_eq!(parsed.request.evidence, evidence.unwrap_or(Value::Null));
        }
    }
    for invalid in [
        json!("yaml"),
        json!(""),
        json!("JSON"),
        Value::Null,
        json!(7),
        json!({}),
    ] {
        let input = json!({"task_id":"format-contract", "stage":"build", "action":"pass", "format":invalid});
        assert!(serde_json::from_value::<TaskSubmitInput>(input).is_err());
    }
    let unknown = json!({"task_id":"format-contract", "stage":"build", "action":"pass", "format":"json", "unknown":true});
    assert!(serde_json::from_value::<TaskSubmitInput>(unknown).is_err());
}
