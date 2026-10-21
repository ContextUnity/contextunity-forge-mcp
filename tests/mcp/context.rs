use crate::common::{
    assertions,
    mcp_client::{invalidate_mcp_connection_cache, run_cli, run_cli_json, StdioClient as Client},
    Workspace,
};
use contextunity_forge_mcp::{
    core::{
        format::OutputFormat,
        response::{ResponsePolicy, MAX_OUTPUT_BYTES},
    },
    engine::scanner,
    mcp::response,
};
use serde_json::{json, Value};
use std::{fs, time::Duration};

#[cfg(unix)]
#[test]
fn stdio_active_claim_routing_rejects_special_files() {
    for kind in ["fifo", "directory", "fifo-alias"] {
        let workspace = Workspace::new();
        workspace.write("forge-mcp.yaml", "tasks_db: .forge/state.sqlite\n");
        let target = workspace.path(".forge/state.sqlite");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        if kind == "directory" {
            fs::create_dir(&target).unwrap();
        } else {
            let fifo = workspace.path(".forge/pipe");
            assert!(std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success());
            if kind == "fifo-alias" {
                std::os::unix::fs::symlink(&fifo, &target).unwrap();
            } else {
                fs::rename(fifo, &target).unwrap();
            }
        }
        let mut client = Client::new(&workspace);
        let (_, result) = client.call("code_map_overview", json!({}));
        assert!(result.to_string().contains("regular file"), "{result}");
        client.payload("code_map_overview", json!({"worktree":"."}));
    }
}

fn query_cli_with_database(
    workspace: &Workspace,
    database: &std::path::Path,
    args: &[&str],
) -> Value {
    let database = database.to_str().expect("database path should be UTF-8");
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["--db", database]);
    full_args.extend_from_slice(args);
    let output = run_cli_json(workspace, &full_args);
    assertions::assert_command_succeeded(&output);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn stdio_compact_navigation_preserves_symbol_and_page_contracts() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\ntasks_db: .forge/tasks.sqlite\n",
    );
    workspace.write(
        "service.py",
        "def entry():\n    return target()\ndef target():\n    return 1\n",
    );
    workspace.write("other.py", "def target():\n    return 2\n");
    workspace.write("README.md", "# Fixture Guide\nquasarneedle appears here.\n");
    fs::create_dir_all(workspace.path("nested")).unwrap();
    workspace.write("nested/helper.py", "def unique_nested():\n    return 3\n");
    let mut client = Client::new(&workspace);

    let parity_cases = [
        ("task_list", vec!["task", "list"], json!({}), "# task_list"),
        (
            "code_map_inspect",
            vec!["query", "inspect", "service.py:entry"],
            json!({"selector":"service.py:entry","show_doc":true,"include_coverage":true,"show_source":false,"limit":100,"detail":"compact"}),
            "# code_map_inspect",
        ),
        (
            "code_map_explain",
            vec!["query", "explain", "service.py:entry"],
            json!({"selector":"service.py:entry","show_doc":true,"include_coverage":true,"show_source":false,"limit":100,"detail":"compact"}),
            "# code_map_explain",
        ),
        (
            "search_docs",
            vec!["docs", "search", "quasarneedle"],
            json!({"query":"quasarneedle","limit":100,"include_excerpt":false}),
            "# search_docs",
        ),
        (
            "get_doc",
            vec!["docs", "get", "README.md"],
            json!({"path_or_id":"README.md","limit":100,"detail":"full"}),
            "# get_doc",
        ),
    ];
    const FRESHNESS_MARKDOWN: &str = concat!(
        "- **freshness**\n",
        "  - **files_checked**: `4`\n",
        "  - **refresh**: `\"none\"`\n",
        "  - **status**: `\"matched\"`\n",
    );
    let strip_top_level_freshness = |text: &str| {
        let mut semantic_text = String::new();
        let mut freshness_block = None;
        let mut lines = text.split_inclusive('\n').peekable();
        while let Some(line) = lines.next() {
            if line.strip_suffix('\n').unwrap_or(line) == "- **freshness**" {
                assert!(
                    freshness_block.is_none(),
                    "multiple top-level freshness blocks"
                );
                let mut block = line.to_owned();
                while lines.peek().is_some_and(|next| next.starts_with("  ")) {
                    block.push_str(lines.next().unwrap());
                }
                freshness_block = Some(block);
            } else {
                semantic_text.push_str(line);
            }
        }
        (semantic_text, freshness_block)
    };
    for (tool, cli_args, arguments, heading) in parity_cases {
        let cli_markdown = run_cli(&workspace, &cli_args);
        assertions::assert_command_succeeded(&cli_markdown);
        let cli_text = std::str::from_utf8(&cli_markdown.stdout).unwrap();
        let cli_text = cli_text.strip_suffix('\n').unwrap_or(cli_text);

        let (_, default_response) = client.call(tool, arguments.clone());
        assert_ne!(
            default_response["result"]["isError"], true,
            "{default_response}"
        );
        let default_text = default_response["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(default_text.lines().next(), Some(heading));

        let cli_json = run_cli_json(&workspace, &cli_args);
        assertions::assert_command_succeeded(&cli_json);
        let cli_json: Value = serde_json::from_slice(&cli_json.stdout).unwrap();
        let mut mcp_json = client.payload(tool, arguments.clone());
        let mcp_freshness = mcp_json
            .as_object_mut()
            .and_then(|result| result.remove("freshness"));
        let (mcp_domain_text, markdown_freshness) = strip_top_level_freshness(default_text);
        let expected_freshness = json!({"status":"matched","files_checked":4,"refresh":"none"});
        if let Some(actual) = &mcp_freshness {
            assert_eq!(actual, &expected_freshness, "{tool} emitted freshness");
        }
        if tool == "task_list" {
            assert!(mcp_freshness.is_none(), "task_list emitted graph freshness");
        }
        assert_eq!(
            markdown_freshness.is_some(),
            mcp_freshness.is_some(),
            "{tool} Markdown freshness must match the actual JSON envelope presence"
        );
        if mcp_freshness.is_some() {
            assert_eq!(
                markdown_freshness.as_deref(),
                Some(FRESHNESS_MARKDOWN),
                "{tool} actual Markdown freshness envelope"
            );
        } else {
            assert!(
                markdown_freshness.is_none(),
                "{tool} unexpected Markdown freshness"
            );
        }
        assert_eq!(cli_json, mcp_json, "{tool} parsed JSON parity");

        assert_eq!(cli_text, mcp_domain_text, "{tool} default Markdown parity");

        let mut markdown_arguments = arguments.clone();
        markdown_arguments["format"] = json!("markdown");
        let (_, explicit_markdown) = client.call(tool, markdown_arguments);
        assert_eq!(
            explicit_markdown["result"]["content"][0]["text"], default_text,
            "{tool} explicit Markdown parity"
        );
    }

    let first = client.payload(
        "code_map_search",
        json!({"pattern":"target","exact":true,"limit":1,"detail":"full"}),
    );
    let page = &first["nodes"];
    assert_eq!(page["has_more"], true);
    assert!(page.get("continuation_hint").is_none());
    let selector = page["items"][0]["id"].as_str().unwrap();
    assert!(page["items"][0]["signature"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert_eq!(
        page["items"][0]["inspect_selector"],
        json!({"selector":selector,"show_source":true})
    );
    assert!(page["items"][0]["source"].as_str().is_some());
    assert!(page["items"][0]["source_preview"].is_object());
    assert!(page["items"][0].get("is_test").is_none());
    assert!(page["items"][0].get("generated").is_none());
    let second = client.payload("code_map_search", json!({"pattern":"target","exact":true,"limit":1,"offset":page["next_offset"],"generation":page["generation"]}));
    assert_eq!(second["nodes"]["has_more"], false);
    assert!(second["nodes"].get("next_offset").is_none());
    assert!(second["nodes"].get("continuation_hint").is_none());
    assert_eq!(second["freshness"]["status"], "matched");
    assert!(second["freshness"].get("output_root").is_none());
    assert!(second["freshness"].get("corpus_hash").is_none());
    assert!(second["freshness"].get("generation").is_none());
    assert!(second["nodes"].get("generation").is_none());
    let inspected = client.payload("code_map_inspect", json!({"selector":selector}));
    assert_eq!(inspected["node"]["id"], selector);
    assert!(inspected.get("documents").is_none());
    let snippet = client.payload("get_code_snippet", json!({"selector":selector}));
    assert!(snippet["header"].as_str().unwrap().contains(&format!(
        "{}:{}-{}",
        snippet["node"]["path"].as_str().unwrap(),
        snippet["source_preview"]["start_line"],
        snippet["source_preview"]["end_line"]
    )));
    let explained = client.payload("code_map_explain", json!({"selector":"service.py:entry"}));
    assert!(explained["relations"]
        .as_array()
        .unwrap()
        .contains(&json!("outgoing")));
    let isolated = client.payload(
        "code_map_explain",
        json!({"selector":"target","path":"other.py"}),
    );
    assert_eq!(isolated["node"]["path"], "other.py");
    assert!(isolated.get("outgoing").is_none());
    let (_, ambiguous) = client.call("code_map_inspect", json!({"selector":"target"}));
    let ambiguous_text = ambiguous["result"]["content"][0]["text"].as_str().unwrap();
    let outcome: serde_json::Value = serde_json::from_str(ambiguous_text)
        .unwrap_or_else(|_| panic!("ambiguity response is not a JSON envelope: {ambiguous_text}"));
    assert_eq!(outcome["outcome"], "ambiguous");
    assert_eq!(outcome["total"], 2);
    let candidates = outcome["candidates"].as_array().unwrap();
    for candidate in candidates {
        for field in ["id", "path", "line", "qualname", "signature"] {
            assert!(
                candidate.get(field).is_some(),
                "missing {field}: {candidate}"
            );
        }
    }
    let scoped = client.payload(
        "code_map_inspect",
        json!({"selector":"target","path":"other.py"}),
    );
    assert_eq!(scoped["node"]["path"], "other.py");
    let scoped_search = client.payload(
        "code_map_search",
        json!({"pattern":"target","exact":true,"path":"other.py"}),
    );
    assert_eq!(scoped_search["nodes"]["total"], 1);
    assert_eq!(scoped_search["nodes"]["items"][0]["path"], "other.py");
    let directory_scoped = client.payload(
        "code_map_inspect",
        json!({"selector":"unique_nested","path":"nested"}),
    );
    assert_eq!(directory_scoped["node"]["path"], "nested/helper.py");
    let snippet_scoped = client.payload(
        "get_code_snippet",
        json!({"selector":"target","path":"other.py"}),
    );
    assert_eq!(snippet_scoped["node"]["path"], "other.py");
    let tests_scoped = client.payload(
        "code_map_tests",
        json!({"selector":"target","path":"other.py"}),
    );
    assert_eq!(tests_scoped["selector"]["path"], "other.py");
    let impact_scoped = client.payload(
        "code_map_impact",
        json!({"selector":"target","path":"other.py","depth":0}),
    );
    assert_eq!(impact_scoped["selector"]["path"], "other.py");
    let removal_scoped = client.payload(
        "code_map_prove_removal",
        json!({"selector":"target","path":"other.py"}),
    );
    assert_eq!(removal_scoped["selected_ids"]["total"], 1);
    let query_scoped = client.payload(
        "code_map_query",
        json!({"operation":"inspect","selector":"target","path":"other.py"}),
    );
    assert_eq!(query_scoped["node"]["path"], "other.py");
    for selector in [
        "service.py:function:entry",
        "service.py::entry",
        "service.py:2",
        "service.py#L2",
    ] {
        let selected = client.payload("code_map_inspect", json!({"selector":selector}));
        assert_eq!(selected["node"]["path"], "service.py", "{selector}");
    }
    let (_, rejected) = client.call(
        "code_map_inspect",
        json!({"selector":"target","pah":"other.py"}),
    );
    assert_eq!(rejected["result"]["isError"], true);
    let rejected: Value =
        serde_json::from_str(rejected["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rejected["error"]["code"], "rejected_arguments");
    assert_eq!(rejected["error"]["fields"], json!(["pah"]));
    let (_, rejected_search) = client.call(
        "code_map_search",
        json!({"pattern":"target","pah":"other.py"}),
    );
    assert_eq!(rejected_search["result"]["isError"], true);
    let rejected_search: Value = serde_json::from_str(
        rejected_search["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rejected_search["error"]["code"], "rejected_arguments");
}

#[test]
fn stdio_search_previews_and_impact_depth_defaults_are_bounded() {
    let workspace = Workspace::new();
    let body_lines = (0..24)
        .map(|line| format!("    value_{line} = {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let long_line = "x".repeat(6000);
    for index in 0..4 {
        workspace.write(
            format!("hit_{index}.py"),
            &format!("def hit_{index}():\n{body_lines}\n    return \"{long_line}\"\n"),
        );
    }
    workspace.write(
        "calls.py",
        "def target():\n    return 1\ndef middle():\n    return target()\ndef outer():\n    return middle()\n",
    );

    let mut client = Client::new(&workspace);
    let ast_guide = client.payload("forge_guide", json!({"topic":"ast"}));
    for capability in [
        "complete_syntax",
        "declaration_without_body",
        "fragment_probe",
        "attribute",
    ] {
        assert!(ast_guide["capabilities"].get(capability).is_some());
    }
    assert!(ast_guide["wildcards"].get("$NAME").is_some());
    assert!(ast_guide["wildcards"].get("$$$SEQ").is_some());
    let (_, tool_list) = client.request("tools/list", json!({}));
    let tools = tool_list["result"]["tools"].as_array().unwrap();
    let ast_tool = tools
        .iter()
        .find(|tool| tool["name"] == "ast_grep_search")
        .unwrap();
    assert!(ast_tool["description"]
        .as_str()
        .unwrap()
        .contains("fragment_probe"));
    let inspect_tool = tools
        .iter()
        .find(|tool| tool["name"] == "code_map_inspect")
        .unwrap();
    let selector_description = inspect_tool["inputSchema"]["properties"]["selector"]["description"]
        .as_str()
        .unwrap();
    for selector_form in [
        "path:name",
        "path:known_kind:name",
        "path:line",
        "path#Lline",
    ] {
        assert!(
            selector_description.contains(selector_form),
            "{selector_description}"
        );
    }

    let without_previews = client.payload(
        "code_map_search",
        json!({"pattern":"hit_*","kind":"function"}),
    );
    assert_eq!(without_previews["nodes"]["total"], 4);
    for hit in without_previews["nodes"]["items"].as_array().unwrap() {
        assert!(hit["signature"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(
            hit["inspect_selector"],
            json!({"selector":hit["id"],"show_source":true})
        );
        assert!(hit.get("source").is_none());
    }

    let with_previews = client.payload(
        "code_map_search",
        json!({"pattern":"hit_*","kind":"function","include_preview":true,"limit":100}),
    );
    assert_eq!(with_previews["nodes"]["total"], 4);
    let mut preview_count = 0;
    let mut preview_bytes = 0;
    for hit in with_previews["nodes"]["items"].as_array().unwrap() {
        if let Some(source) = hit["source"].as_str() {
            preview_count += 1;
            preview_bytes += source.len();
            assert!(source.lines().count() <= 20);
            assert!(hit["source_preview"].is_object());
        }
    }
    assert_eq!(preview_count, 3);
    assert!(preview_bytes <= 12 * 1024, "{preview_bytes} preview bytes");

    let impact = client.payload("code_map_impact", json!({"selector":"calls.py:target"}));
    let generic_impact = client.payload(
        "code_map_query",
        json!({"operation":"impact","selector":"calls.py:target"}),
    );
    assert_eq!(impact["depth"], 1);
    assert_eq!(generic_impact["depth"], 2);
    let names = |value: &Value| {
        value["nodes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    assert!(!names(&impact).iter().any(|name| name == "outer"));
    assert!(names(&generic_impact).iter().any(|name| name == "outer"));

    let db = workspace.path(".forge/cli.sqlite");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let run_cli = |args: &[&str]| query_cli_with_database(&workspace, &db, args);
    let cli_impact = run_cli(&["query", "impact", "calls.py:target"]);
    let cli_generic_impact = run_cli(&["query", "run", "impact", "calls.py:target"]);
    assert_eq!(cli_impact["depth"], 1);
    assert_eq!(cli_generic_impact["depth"], 2);
    assert!(!names(&cli_impact).iter().any(|name| name == "outer"));
    assert!(names(&cli_generic_impact)
        .iter()
        .any(|name| name == "outer"));
}

#[test]
fn cli_selector_commands_apply_workspace_path_scope() {
    let workspace = Workspace::new();
    workspace.write("service.py", "def target():\n    return 1\n");
    workspace.write("other.py", "def target():\n    return 2\n");
    let forge_dir = workspace.path(".forge");
    fs::create_dir_all(&forge_dir).unwrap();
    let db = forge_dir.join("cli.sqlite");
    let run_cli = |args: &[&str]| query_cli_with_database(&workspace, &db, args);

    let inspected = run_cli(&["query", "inspect", "target", "--path", "other.py"]);
    assert_eq!(inspected["node"]["path"], "other.py");
    let explained = run_cli(&["query", "explain", "target", "--path", "other.py"]);
    assert_eq!(explained["node"]["path"], "other.py");
    let impacted = run_cli(&[
        "query", "impact", "target", "--depth", "0", "--path", "other.py",
    ]);
    assert_eq!(impacted["selector"]["path"], "other.py");
    let tested = run_cli(&["query", "tests", "target", "--path", "other.py"]);
    assert_eq!(tested["selector"]["path"], "other.py");
    let removed = run_cli(&["query", "remove", "target", "--path", "other.py"]);
    assert_eq!(removed["selected_ids"]["total"], 1);
}

#[test]
fn compact_symbol_page_has_stable_lean_serialization() {
    let items: Vec<Value> = (0..30)
        .map(|i| {
            let name = format!("public_application_entrypoint_{i}");
            json!({"id":format!("function:{i}"),"kind":"function","name":name,"qualname":name,"path":"a.py","is_test":0,"generated":false})
        })
        .collect();
    let mut payload = json!({"generation":"a".repeat(64),"nodes":{"items":items,"total":30,"offset":0,"limit":30,"has_more":false,"generation":"a".repeat(64),"next_offset":null,"continuation_hint":null}});
    let original_bytes = payload.to_string().len();
    response::compact_mcp_metadata(&mut payload);
    assert!(payload.to_string().len() * 100 <= original_bytes * 65);
    assert_eq!(payload["nodes"]["items"].as_array().unwrap().len(), 30);
    assert!(payload["nodes"].get("generation").is_none());
    assert!(payload.get("generation").is_none());
    let stable = payload.clone();
    response::compact_mcp_metadata(&mut payload);
    assert_eq!(payload, stable);
    let mut flagged = json!({"nodes":{"items":[{"id":"test:1","kind":"function","name":"test","qualname":"suite.test","path":"test.py","is_test":1,"generated":true}]}});
    response::compact_mcp_metadata(&mut flagged);
    assert_eq!(flagged["nodes"]["items"][0]["is_test"], true);
    assert_eq!(flagged["nodes"]["items"][0]["generated"], true);
    assert_eq!(flagged["nodes"]["items"][0]["qualname"], "suite.test");
}

#[test]
fn stdio_zero_match_search_has_compact_page() {
    let workspace = Workspace::new();
    workspace.write("service.py", "def useful_symbol():\n    return 1\n");
    let mut client = Client::new(&workspace);
    let query = json!({"pattern":"*noise*","path":"service.py"});
    client.payload("code_map_search", query.clone());
    let payload = client.payload("code_map_search", query);
    assert_eq!(
        payload,
        json!({"pattern":"*noise*","nodes":{"total":0,"items":[]}})
    );
}

#[test]
fn compact_incomplete_page_keeps_limit_evidence() {
    for flag in ["computation_truncated", "byte_limited"] {
        let mut payload = json!({"matches":{
            "total":null,"offset":0,"limit":30,"has_more":false,
            "generation":"123456789abcdef","items":[],
            "continuation_hint":"Narrow the pattern"
        }});
        payload["matches"][flag] = json!(true);
        response::compact_mcp_metadata(&mut payload);
        assert_eq!(payload["matches"][flag], true);
        assert_eq!(payload["matches"]["generation"], "12345678");
        assert_eq!(
            payload["matches"]["continuation_hint"],
            "Narrow the pattern"
        );
    }
}
#[cfg(feature = "lang-rust")]
#[test]
fn stdio_distinguishes_known_external_imports_from_missing_sources() {
    let workspace = Workspace::new();
    workspace.write("main.rs", "use std::fmt::Debug;\nuse missing::Thing;\n");
    let mut client = Client::new(&workspace);
    let overview = client.payload("code_map_overview", json!({}));
    assert_eq!(overview["counts"]["external_imports"], 2);
    assert_eq!(overview["counts"]["unresolved"], 0);
    let analysis = client.payload("code_map_analyze", json!({"target":"main.rs"}));
    assert_eq!(analysis["total_external_imports"], 2);
    assert_eq!(analysis["total_unresolved"], 0);
    assert_eq!(
        analysis["external_imports"]["items"][0]["expression"],
        "std::fmt::Debug"
    );
    assert_eq!(
        analysis["external_imports"]["items"][1]["expression"],
        "missing::Thing"
    );
    assert!(analysis["resolution"]["items"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn stdio_budget_and_query_errors_explain_how_to_retry() {
    let workspace = Workspace::new();
    workspace.write("README.md", "# Indexed document\n");
    let mut client = Client::new(&workspace);
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
            "supported: overview,inspect,explain,impact,slice,unwired,sql",
        ),
        (
            "code_map_inspect",
            json!({"selector":"file:nonexistent.rs"}),
            "File paths are passed without a file: prefix",
        ),
    ] {
        let (_, response) = client.call(name, arguments);
        assert_eq!(response["result"]["isError"], true, "{response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains(expected), "{name}: {message}");
    }
}
#[test]
fn stdio_analyze_preserves_sql_columns_named_like_metadata() {
    let workspace = Workspace::new();
    workspace.write("main.py", "def main(): pass\n");
    let mut client = Client::new(&workspace);
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
    );
    assert_eq!(result["rows"]["items"][0]["generation"], "1234567é");
    assert_eq!(result["rows"]["items"][0]["output_root"], "0123456789");
    assert_eq!(
        result["rows"]["items"][0]["corpus_hash"],
        "abcdef0123456789"
    );
    assert_eq!(
        result["rows"]["items"][0]["details"]["nodes"]["items"][0]["output_root"],
        "node-root-12345"
    );
    assert_eq!(
        result["rows"]["items"][0]["details"]["symbols"][0]["corpus_hash"],
        "symbol-hash-1234"
    );
    assert_eq!(
        result["rows"]["items"][0]["details"]["doc_sections"][0]["output_root"],
        "section-root-12"
    );
    assert_eq!(
        result["rows"]["items"][0]["details"]["freshness"]["corpus_hash"],
        "nested-hash-12345"
    );
    assert_eq!(
        result["rows"]["items"][0]["details"]["metadata"]["output_root"],
        "nested-meta-1234"
    );

    // Real SQL values are opaque even when they resemble operation metadata.
    let sql = client.payload("code_map_analyze", json!({"format":"json",
        "target":"SELECT 'sql-generation-must-remain-complete' AS generation, 0 AS count, json('false') AS complete, json('[]') AS items"}));
    assert_eq!(
        sql["rows"]["items"][0]["generation"],
        "sql-generation-must-remain-complete"
    );
    assert_eq!(sql["rows"]["items"][0]["count"], 0);
    assert_eq!(sql["rows"]["items"][0]["items"], "[]");

    assert_eq!(sql["rows"]["items"][0]["complete"], "false");
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
}
#[test]
fn stdio_analyze_rejects_oversized_intermediate_sql_values() {
    let workspace = Workspace::new();
    workspace.write("main.py", "def main(): pass\n");
    let mut client = Client::new(&workspace);
    let exact = client.payload(
        "code_map_analyze",
        json!({"target":"SELECT length(randomblob(8388608)) AS payload"}),
    );
    assert_eq!(exact["rows"]["items"][0]["payload"], 8388608);
    let (_, response) = client.call(
        "code_map_analyze",
        json!({"target":"SELECT length(randomblob(8388609)) AS payload"}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
}
#[test]
fn cold_build_populates_document_fts_search() {
    let workspace = Workspace::new();
    workspace.write(
        "guide.md",
        "# Guide\nThis document contains quasarneedle for search.\n",
    );
    let mut client = Client::new(&workspace);
    let result = client.payload(
        "code_map_analyze",
        json!({"target":"SELECT count(*) AS matches FROM doc_search WHERE doc_search MATCH 'quasarneedle'"}),
    );
    assert_eq!(result["rows"]["items"][0]["matches"], 1);
}
#[test]
fn stdio_caps_documents_scalars_checkpoints_and_tool_errors() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  max_output_bytes: 1024\n",
    );
    workspace.write(
        "README.md",
        &format!("# Huge document\n{}", "\"\\Привіт\t".repeat(30_000)),
    );
    let mut client = Client::new(&workspace);
    let (_, tools) = client.request("tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() >= 15);
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
        let (_, response) = client.call(name, arguments);
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
        let name = format!("invalid-format-{index}");
        let (bytes, response) = client.call(
            "session_checkpoint",
            json!({
                "format":invalid_format,
                "action":"save",
                "name":name,
                "content":{"must_not_persist":true}
            }),
        );
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
                .get(format!("invalid-format-{index}"))
                .is_none(),
            "invalid format persisted checkpoint invalid-format-{index}"
        );
    }
    let (_, unknown) = client.call(&"\"\\Привіт".repeat(30_000), json!({}));
    assert!(unknown["result"]["isError"] == true || unknown.get("error").is_some());
    client.payload(
        "session_checkpoint",
        json!({"action":"save","name":"large","content":"\"\\Привіт".repeat(20_000)}),
    );
    let (_, response) = client.call("session_checkpoint", json!({"action":"get","name":"large"}));
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        client.payload("session_checkpoint", json!({"action":"list"})),
        json!({"large":{"bytes":320002}})
    );
    let (_, response) = client.call("get_doc", json!({"path_or_id":"README.md","limit":101}));
    assert_eq!(response["result"]["isError"], true);
}
#[test]
fn stdio_byte_pruning_continues_after_exactly_the_emitted_rows() {
    let workspace = Workspace::new();
    workspace.write("README.md", "# Small\nindexed\n");
    let mut client = Client::new(&workspace);
    let sql = "WITH RECURSIVE t(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM t WHERE n<16) SELECT n,replace(hex(zeroblob(1800)), '0', char(34)) AS escaped FROM t ORDER BY n";
    let mut offset = 0;
    let mut generation = Value::Null;
    let mut seen = Vec::new();
    loop {
        let payload = client.payload(
            "code_map_analyze",
            json!({"target":sql,"limit":30,"offset":offset,"generation":generation}),
        );
        let page = &payload["rows"];
        assert_eq!(page["total"], 17);
        let items = page["items"].as_array().unwrap();
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
}
#[test]
fn adapter_policy_applies_without_changing_fact_identity_and_rejects_invalid_bounds() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  page_size: 2\n  max_output_bytes: 4096\n",
    );
    workspace.write("README.md", "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n");
    let first = scanner::load_adapter(workspace.root(), None).unwrap();
    let mut client = Client::new(&workspace);
    let payload = client.payload("get_doc", json!({"path_or_id":"README.md"}));
    assert_eq!(payload["sections"]["items"].as_array().unwrap().len(), 2);
    let database = workspace.db();
    let before = fs::metadata(&database).unwrap().modified().unwrap();
    workspace.write("forge-mcp.yaml", "roots: [.]\nresponse:\n  detail: full\n  page_size: 3\n  max_output_bytes: 2048\n  source_context:\n    max_body_lines: 10\n");
    let second = scanner::load_adapter(workspace.root(), None).unwrap();
    assert_eq!(first.digest, second.digest);
    let (bytes, response) =
        client.call("get_doc", json!({"path_or_id":"README.md","format":"json"}));
    assert!(bytes <= 2048, "{bytes}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(fs::metadata(database).unwrap().modified().unwrap(), before);
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  max_output_bytes: 1024\n",
    );
    let (bytes, response) = client.call(
        "code_map_search",
        json!({"pattern":"*","detail":"A".repeat(10_000)}),
    );
    assert!(bytes <= 1024, "parameter error frame was {bytes} bytes");
    assert_eq!(response["result"]["isError"], true);
    for settings in [
        "page_size: 0",
        "page_size: 101",
        "max_output_bytes: 65537",
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
fn stdio_default_pages_and_stale_continuation_are_explicit() {
    let workspace = Workspace::new();
    let source = (0..125)
        .map(|i| format!("def symbol_{i:03}():\n    return {i}\n"))
        .collect::<String>();
    workspace.write("service.py", &source);
    let body = (0..12)
        .map(|i| format!("    value_{i} = {i}\n"))
        .collect::<String>();
    workspace.write(
        "preview.py",
        &format!("def selected():\n{body}    return 0\n"),
    );
    let mut client = Client::new(&workspace);
    let initial = client.payload(
        "code_map_search",
        json!({"pattern":"symbol_*","kind":"function"}),
    );
    let generation = initial["nodes"]["generation"].as_str().unwrap();
    assert!(!generation.is_empty());
    assert_eq!(initial["nodes"]["limit"], 30);
    assert_eq!(initial["nodes"]["items"].as_array().unwrap().len(), 30);
    assert_eq!(initial["nodes"]["total"], 125);
    let next = client.payload("code_map_search", json!({"pattern":"symbol_*","kind":"function","limit":100,"offset":30,"generation":initial["nodes"]["generation"]}));
    assert_eq!(next["nodes"]["items"].as_array().unwrap().len(), 95);
    assert_eq!(next["nodes"]["has_more"], false);
    let mut colliding_prefix = generation.to_owned();
    colliding_prefix.replace_range(0..1, if &generation[0..1] == "0" { "1" } else { "0" });
    let (_, stale_prefix) = client.call(
        "code_map_search",
        json!({"pattern":"symbol_*","limit":100,"offset":30,"generation":colliding_prefix}),
    );
    assert_eq!(stale_prefix["result"]["isError"], true);
    let (_, missing) = client.call("code_map_search", json!({"pattern":"symbol_*","offset":30}));
    assert_eq!(missing["result"]["isError"], true);
    workspace.write("service.py", &format!("{source}\ndef added(): return 0\n"));
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let (_, stale) = client.call(
        "code_map_search",
        json!({"pattern":"symbol_*","offset":30,"generation":initial["nodes"]["generation"]}),
    );
    assert_eq!(stale["result"]["isError"], true);

    let listed = client.request_value("tools/list", json!({}));
    for tool in listed["result"]["tools"].as_array().unwrap() {
        let format = &tool["inputSchema"]["properties"]["format"];
        let schema = format.to_string();
        assert!(
            schema.contains("markdown") && schema.contains("json"),
            "tool {} omits format selection: {format}",
            tool["name"]
        );
    }
    // Source-only continuation must retain a usable token with every collection complete.
    let source = client.payload(
        "get_code_snippet",
        json!({"selector":"selected",
        "max_body_lines":3,"format":"json"}),
    );
    let offset = source["source_preview"]["next_source_offset"]
        .as_u64()
        .unwrap();
    assert!(offset > 0, "{source}");
    let token = source["generation"]
        .as_str()
        .expect("source continuation requires generation");
    let resumed = client.payload(
        "get_code_snippet",
        json!({"selector":"selected",
        "max_body_lines":3,"source_offset":offset,"generation":token,"format":"json"}),
    );
    assert_eq!(resumed["source_preview"]["body_offset"], offset);
    let (_, stale) = client.call(
        "get_code_snippet",
        json!({"selector":"selected",
        "source_offset":offset,"generation":"stale-token","format":"json"}),
    );
    assert_eq!(stale["result"]["isError"], true, "{stale}");
}
#[cfg(feature = "lang-python")]
#[test]
fn stdio_symbol_search_groups_the_current_page_by_file() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        "def target(): return 1\ndef target_helper(): return target()\n",
    );
    let mut client = Client::new(&workspace);
    for (pattern, expected) in [("target", 1), ("target*", 2)] {
        let response = client.payload(
            "code_map_search",
            json!({"pattern":pattern,"kind":"function","group_by_file":true}),
        );
        assert_eq!(response["nodes"]["total"], expected, "{pattern}");
        assert_eq!(response["nodes"]["items"], json!([]));
        assert_eq!(
            response["nodes"]["grouped_by_file"]["service.py"]
                .as_array()
                .unwrap()
                .len(),
            expected,
            "{pattern}"
        );
    }
}
#[cfg(feature = "lang-python")]
#[test]
fn stdio_source_previews_respect_boundaries_and_continue_crlf_utf8() {
    let workspace = Workspace::new();
    let body = (0..90)
        .map(|i| format!("    value_{i} = 'Привіт'\r\n"))
        .collect::<String>();
    let source = format!("def previous():\r\n    return 'secret'\r\n\r\n# selected\r\ndef selected():\r\n{body}    return value_0\r\ndef following(): return 'hidden'\r\n");
    workspace.write("service.py", &source);
    let mut client = Client::new(&workspace);
    let default = client.payload("code_map_inspect", json!({"selector":"selected"}));
    assert!(default.get("source").is_none());
    let explanation = client.payload(
        "code_map_explain",
        json!({"selector":"selected","show_source":true}),
    );
    assert!(explanation["source"]
        .as_str()
        .unwrap()
        .contains("def selected"));
    assert!(explanation.get("incoming").is_some());
    assert_eq!(explanation["relations"], json!(["incoming"]));
    let first = client.payload(
        "get_code_snippet",
        json!({"selector":"selected","max_body_lines":35}),
    );
    let text = first["source"].as_str().unwrap();
    assert!(text.contains("Привіт\r\n") || text.contains("Привіт'\r\n"));
    assert!(!text.contains("previous"));
    assert!(!text.contains("following"));
    assert_eq!(first["source_preview"]["body_lines"], 35);
    let second = client.payload("get_code_snippet", json!({"selector":"selected","max_body_lines":35,"source_offset":first["source_preview"]["next_source_offset"],"generation":first["generation"]}));
    assert_eq!(second["source_preview"]["body_offset"], 35);
    assert!(!second["source"].as_str().unwrap().contains("def selected"));
    workspace.write(
        "huge.py",
        &format!("def enormous(): return '{}'\n", "Привіт".repeat(50_000)),
    );
    invalidate_mcp_connection_cache(&workspace.db()).unwrap();
    let enormous = client.payload("get_code_snippet", json!({"selector":"enormous"}));
    assert_eq!(enormous["source_preview"]["truncated_line"], true);
    assert!(enormous["source_preview"]["next_source_offset"].is_null());
    assert!(enormous["source_preview"]["continuation_hint"]
        .as_str()
        .unwrap()
        .contains("huge.py"));
}
#[cfg(feature = "lang-python")]
#[path = "../mcp_context/ast_search.rs"]
mod ast_search;
#[cfg(feature = "lang-python")]
#[test]
fn repair_ast_horizon_has_no_unusable_cursor() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        &(0..10_001)
            .map(|i| format!("print({i})\n"))
            .collect::<String>(),
    );
    let mut client = Client::new(&workspace);
    let first = client.payload(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","limit":100}),
    );
    let last = client.payload("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","offset":9900,"limit":100,"generation":first["matches"]["generation"]}));
    let page = &last["matches"];
    assert_eq!(page["items"].as_array().unwrap().len(), 100);
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
}
#[cfg(feature = "lang-python")]
#[test]
fn repair_ast_byte_pruning_keeps_earlier_horizon_continuation_usable() {
    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "response:\n  max_output_bytes: 4096\n");
    workspace.write(
        "service.py",
        &(0..10_001)
            .map(|i| format!("print({i})\n"))
            .collect::<String>(),
    );
    let mut client = Client::new(&workspace);
    let first = client.payload(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","limit":100}),
    );
    let mut offset = 9900;
    let mut lines = Vec::new();
    loop {
        let payload = client.payload("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","offset":offset,"limit":100,"generation":first["matches"]["generation"]}));
        let page = &payload["matches"];
        let items = page["items"].as_array().unwrap();
        assert!(!items.is_empty());
        lines.extend(items.iter().map(|item| item["line"].as_u64().unwrap()));
        if page["next_offset"].is_null() {
            assert_eq!(page["computation_truncated"], true);
            assert_eq!(lines.last(), Some(&10_000));
            break;
        }
        let next = page["next_offset"].as_u64().unwrap();
        assert_eq!(next, offset + items.len() as u64);
        assert!(next > offset && next < 10_000);
        offset = next;
    }
    assert_eq!(lines, (9901..=10_000).collect::<Vec<_>>());
}
#[cfg(feature = "lang-python")]
#[test]
fn repair_variadic_matcher_budget_is_checked_inside_backtracking() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        &format!("f({})\nmissing = 0\n", vec!["0"; 400].join(",")),
    );
    let mut client = Client::new(&workspace);
    let started = std::time::Instant::now();
    let payload = client.payload(
        "ast_grep_search",
        json!({"pattern":"f($$$A, $$$B, $$$C, missing)","language":"python","limit":1}),
    );
    let elapsed = started.elapsed();
    eprintln!("MCP-02: variadic missing-terminal stdio response completed in {elapsed:?}");
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
        let ordinary = client.payload(
            "ast_grep_search",
            json!({"pattern":pattern,"language":"python","limit":100}),
        );
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
}
#[test]
fn repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged() {
    let workspace = Workspace::new();
    let mut client = Client::new(&workspace);
    let page = json!({"total":2,"offset":0,"limit":2,"generation":"saved-snapshot","next_offset":null,"has_more":false,"items":[{"text":"x".repeat(40_000)},{"text":"y".repeat(40_000)}]});
    for (name, value) in [
        ("page", page.clone()),
        ("nested", json!({"snapshot":{"matches":page}})),
    ] {
        client.payload(
            "session_checkpoint",
            json!({"action":"save","name":name,"content":value}),
        );
        let stored = fs::read(workspace.path(".forge/checkpoints.json")).unwrap();
        let persisted: Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(persisted["entries"][name], value);
        for format in [None, Some("markdown"), Some("json")] {
            let mut arguments = json!({"action":"get","name":name});
            if let Some(format) = format {
                arguments["format"] = json!(format);
            }
            let (_, response) = client.call("session_checkpoint", arguments);
            assert_eq!(
                response["result"]["isError"], true,
                "oversized {name} {format:?} get created a fabricated page"
            );
            assert!(response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(".forge/checkpoints.json"));
            assert_eq!(
                fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
                stored
            );
        }
        let listed = client.payload("session_checkpoint", json!({"action":"list"}));
        assert_eq!(
            listed[name]["bytes"],
            serde_json::to_vec(&value).unwrap().len()
        );
        assert_eq!(
            fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
            stored
        );
    }
    let small = json!({"total":2,"offset":0,"limit":2,"generation":"saved","next_offset":null,"items":[1,2]});
    client.payload(
        "session_checkpoint",
        json!({"action":"save","name":"small","content":small}),
    );
    assert_eq!(
        client.payload("session_checkpoint", json!({"action":"get","name":"small"})),
        small
    );

    let opaque = json!({
        "generation":"checkpoint-generation-must-remain-complete",
        "freshness":{"output_root":"user-root-must-remain-complete","refresh":"none"},
        "metadata":{},"items":[],"count":0,"complete":false,
        "nested":{"total":0,"offset":0,"limit":1,"generation":"user-page-token","items":[]}
    });
    let (_, saved) = client.call(
        "session_checkpoint",
        json!({
            "action":"save","name":"format-contract","content":opaque
        }),
    );
    assert_ne!(saved["result"]["isError"], true, "{saved}");
    let persisted = fs::read(workspace.path(".forge/checkpoints.json")).unwrap();

    // Invalid presentation must be rejected before a real persisted mutation.
    for invalid in ["yaml", "", "JSON"] {
        let (_, rejected) = client.call(
            "session_checkpoint",
            json!({
                "action":"save","name":"format-contract","content":{"changed":true},"format":invalid
            }),
        );
        assert!(
            rejected.get("error").is_some() || rejected["result"]["isError"] == true,
            "invalid MCP format {invalid:?}: {rejected}"
        );
        assert_eq!(
            fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
            persisted
        );
        let rejected = run_cli(
            &workspace,
            &[
                "--format",
                invalid,
                "checkpoint",
                "save",
                "--name",
                "format-contract",
                "--content",
                "{\"changed\":true}",
            ],
        );
        assert!(!rejected.status.success(), "invalid CLI format {invalid:?}");
        assert_eq!(
            fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
            persisted
        );
    }

    let mut markdown = Vec::new();
    for format in [None, Some("markdown"), Some("json")] {
        let mut arguments = json!({"action":"get","name":"format-contract"});
        let mut cli_args = Vec::new();
        if let Some(format) = format {
            arguments["format"] = json!(format);
            cli_args.extend(["--format", format]);
        }
        cli_args.extend(["checkpoint", "get", "--name", "format-contract"]);
        let (_, result) = client.call("session_checkpoint", arguments);
        assert_ne!(result["result"]["isError"], true, "{result}");
        let mcp_text = result["result"]["content"][0]["text"].as_str().unwrap();
        let cli = run_cli(&workspace, &cli_args);
        assertions::assert_command_succeeded(&cli);
        let cli_text = std::str::from_utf8(&cli.stdout).unwrap().trim_end();
        if format == Some("json") {
            for text in [mcp_text, cli_text] {
                assert_eq!(serde_json::from_str::<Value>(text).unwrap(), opaque);
            }
        } else {
            for (surface, text) in [("MCP", mcp_text), ("CLI", cli_text)] {
                assert!(
                    serde_json::from_str::<Value>(text).is_err(),
                    "{surface} {format:?} must render Markdown, received raw JSON: {text}"
                );
                for preserved in [
                    "checkpoint-generation-must-remain-complete",
                    "user-root-must-remain-complete",
                    "user-page-token",
                    "false",
                    "0",
                    "**metadata**: `{}`",
                    "**items**: `[]`",
                    "**nested**",
                    "  - **generation**: `\"user-page-token\"`",
                    "  - **items**: `[]`",
                ] {
                    assert!(
                        text.contains(preserved),
                        "{surface} lost {preserved}: {text}"
                    );
                }
            }
            assert_eq!(mcp_text.trim_end(), cli_text);
            markdown.push(mcp_text.to_owned());
        }
    }
    assert_eq!(
        markdown[0], markdown[1],
        "omission and explicit Markdown differ"
    );
    assert_eq!(
        fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
        persisted
    );

    let mut missing_renderings = Vec::new();
    for (name, value, rendered) in [
        ("empty-array", json!([]), "[]"),
        ("empty-object", json!({}), "{}"),
    ] {
        let (_, saved) = client.call(
            "session_checkpoint",
            json!({"action":"save","name":name,"content":value}),
        );
        assert_ne!(saved["result"]["isError"], true, "{saved}");
        let persisted = fs::read(workspace.path(".forge/checkpoints.json")).unwrap();
        let stored: Value = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(stored["entries"][name], value);

        let mut markdown = Vec::new();
        for format in [None, Some("markdown"), Some("json")] {
            let mut arguments = json!({"action":"get","name":name});
            let mut cli_args = Vec::new();
            if let Some(format) = format {
                arguments["format"] = json!(format);
                cli_args.extend(["--format", format]);
            }
            cli_args.extend(["checkpoint", "get", "--name", name]);
            let (_, response) = client.call("session_checkpoint", arguments);
            assert_ne!(response["result"]["isError"], true, "{response}");
            let mcp_text = response["result"]["content"][0]["text"]
                .as_str()
                .unwrap();
            let cli = run_cli(&workspace, &cli_args);
            assertions::assert_command_succeeded(&cli);
            let cli_text = std::str::from_utf8(&cli.stdout).unwrap().trim_end();
            if format == Some("json") {
                for text in [mcp_text, cli_text] {
                    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), value);
                }
            } else {
                for (surface, text) in [("MCP", mcp_text), ("CLI", cli_text)] {
                    if !text.contains(rendered) {
                        missing_renderings.push(format!(
                            "{surface} {format:?} omitted root checkpoint {name}: {text}"
                        ));
                    }
                }
                assert_eq!(mcp_text.trim_end(), cli_text);
                markdown.push(mcp_text.to_owned());
            }
            assert_eq!(
                fs::read(workspace.path(".forge/checkpoints.json")).unwrap(),
                persisted
            );
        }
        assert_eq!(
            markdown[0], markdown[1],
            "empty {name} omission and explicit Markdown differ"
        );
    }
    assert!(
        missing_renderings.is_empty(),
        "empty checkpoint roots were omitted: {}",
        missing_renderings.join("; ")
    );
}
#[tokio::test]
async fn transport_accepts_exact_bound_and_replaces_oversized_frames() {
    use tokio::io::AsyncWriteExt;
    for size in [MAX_OUTPUT_BYTES - 1, MAX_OUTPUT_BYTES] {
        let mut writer = response::BoundedWriter::new(tokio::io::sink());
        let mut frame = vec![b'x'; size - 1];
        frame.push(b'\n');
        writer.write_all(&frame).await.unwrap();
        writer.flush().await.unwrap();
    }
    let mut output = Vec::new();
    let mut writer = response::BoundedWriter::new(&mut output);
    writer
        .write_all(&vec![b'x'; MAX_OUTPUT_BYTES])
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    assert!(output.len() < MAX_OUTPUT_BYTES);
    let error: Value = serde_json::from_slice(&output).unwrap();
    assert!(error.get("error").is_some());
}
#[test]
fn serialization_counts_second_escaping_and_bounds_a_single_item() {
    let policy = ResponsePolicy::default();
    let result = response::result_with_format(
        Ok(
            json!({"nodes":{"total":2,"offset":0,"limit":30,"items":[{"id":"first","text":"\"\\\n".repeat(4000)},{"id":"second","text":"\"\\\n".repeat(4000)}],"has_more":false,"next_offset":null,"generation":"snapshot"}}),
        ),
        &policy,
        OutputFormat::Json,
    );
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let wire = serde_json::to_value(result).unwrap();
    let payload: Value =
        serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(payload["nodes"]["next_offset"], 1);
    let result = response::result_with_format(
        Ok(json!({"scalar":"\"\\Привіт".repeat(30_000)})),
        &policy,
        OutputFormat::Json,
    );
    assert_eq!(result.is_error, Some(true));
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);
}
#[test]
fn stdio_symbol_search_matches_methods_and_functions_interchangeably() {
    let workspace = Workspace::new();
    std::fs::create_dir_all(workspace.path("src")).unwrap();
    workspace.write(
        "src/lib.rs",
        "pub struct Server;\nimpl Server {\n    pub fn admit(&self) {}\n}\n",
    );
    let mut client = Client::new(&workspace);
    let result = client.payload(
        "code_map_search",
        json!({"pattern":"admit*","kind":"method","path":"src/lib.rs"}),
    );
    assert_eq!(result["nodes"]["total"], 1);
    assert_eq!(result["nodes"]["items"][0]["name"], "admit");
    let result_fn = client.payload(
        "code_map_search",
        json!({"pattern":"admit*","kind":"function","path":"src/lib.rs"}),
    );
    assert_eq!(result_fn["nodes"]["total"], 1);
    assert_eq!(result_fn["nodes"]["items"][0]["name"], "admit");
}

#[test]
fn stdio_worktree_parameter_and_active_claim_auto_resolution() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots:\n  - src\ntasks_db: .forge/tasks.sqlite\n",
    );
    std::fs::create_dir_all(workspace.path("src")).unwrap();
    workspace.write("src/main.rs", "pub fn primary_func() {}\n");

    let worktree_dir = workspace.path(".worktrees/021-task");
    std::fs::create_dir_all(worktree_dir.join("src")).unwrap();
    std::fs::write(
        worktree_dir.join("src/semantic.rs"),
        "pub fn semantic_worktree_func() {}\n",
    )
    .unwrap();
    let selected_config = worktree_dir.join("forge-mcp.yaml");
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();

    let mut client = Client::new(&workspace);

    let get_workspace_root = |v: &Value| -> String {
        v["metadata"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["key"] == "workspace_root")
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // 1. Without claim and without worktree parameter -> returns primary workspace
    let overview_primary = client.payload(
        "code_map_overview",
        json!({"aspects":["counts","metadata"]}),
    );
    assert_eq!(
        get_workspace_root(&overview_primary),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // 2. Explicit worktree parameter -> scopes to worktree and auto-indexes
    let wt_str = worktree_dir.to_str().unwrap();
    let selected_index = worktree_dir.join(".forge/code-map.sqlite");
    assert!(!selected_index.exists());
    let invalid_formats = [
        ("escaping string", json!("\0".repeat(2_000))),
        ("non-string", json!(false)),
    ];
    let checkpoint_path = workspace.path(".forge/checkpoints.json");
    let checkpoint_before = fs::read(&checkpoint_path).ok();
    let mut invalid_policy_failures = Vec::new();
    std::fs::write(
        &selected_config,
        "roots: [src]\nresponse:\n  max_output_bytes: 1024\n",
    )
    .unwrap();
    for (label, invalid_format) in &invalid_formats {
        let mut arguments = json!({"worktree":wt_str});
        arguments["format"] = invalid_format.clone();
        let (wire_bytes, response) = client.call("code_map_overview", arguments);
        let is_error = response["result"]["isError"] == true;
        let error_code = response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if wire_bytes > 1024 || !is_error || error_code != "INVALID_FORMAT" {
            invalid_policy_failures.push(format!(
                "explicit selected root {label}: {wire_bytes} wire bytes, isError={is_error}, code={error_code:?}"
            ));
        }
        if selected_index.exists() {
            invalid_policy_failures.push(format!(
                "explicit selected root {label} created its index before rejection"
            ));
        }
        if fs::read(&checkpoint_path).ok() != checkpoint_before {
            invalid_policy_failures.push(format!(
                "explicit selected root {label} changed checkpoint bytes before rejection"
            ));
        }
    }
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();

    let overview_wt = client.payload(
        "code_map_overview",
        json!({"worktree": wt_str, "aspects":["counts","metadata"]}),
    );
    assert_eq!(
        get_workspace_root(&overview_wt),
        worktree_dir.canonicalize().unwrap().to_str().unwrap()
    );

    // Search in worktree finds semantic_worktree_func
    let search_wt = client.payload(
        "code_map_search",
        json!({"pattern": "semantic_worktree_func", "exact": true, "worktree": wt_str}),
    );
    assert_eq!(search_wt["nodes"]["total"], 1);
    assert_eq!(
        search_wt["nodes"]["items"][0]["name"],
        "semantic_worktree_func"
    );

    for alias in ["021-task", "task"] {
        let result = client.payload(
            "code_map_search",
            json!({"pattern":"semantic_worktree_func", "exact":true, "worktree":alias}),
        );
        assert_eq!(result["nodes"]["total"], 1, "alias {alias}: {result}");
    }

    // Search in primary does NOT find semantic_worktree_func
    let search_primary = client.payload(
        "code_map_search",
        json!({"pattern": "semantic_worktree_func", "exact": true}),
    );
    assert_eq!(search_primary["nodes"]["total"], 0);

    // 3. Invalid worktree parameter -> returns tool error
    for invalid in ["nonexistent_dir_12345", "", "src/main.rs"] {
        let (_, invalid_res) = client.call("code_map_overview", json!({"worktree": invalid}));
        assert_eq!(invalid_res["result"]["isError"], true);
        assert!(
            invalid_res["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("WORKTREE_NOT_FOUND"),
            "{invalid_res}"
        );
    }

    // 4. Active claimed task auto-resolution:
    let tasks_db_path = workspace.path(".forge/tasks.sqlite");
    std::fs::create_dir_all(tasks_db_path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(&tasks_db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks(task_id TEXT PRIMARY KEY, milestone_ref TEXT, descriptor TEXT, status TEXT);
         CREATE TABLE task_claims(task_id TEXT, revision INTEGER, worker_id TEXT, worktree TEXT, claimed_at INTEGER, ended INTEGER, PRIMARY KEY(task_id, revision));"
    ).unwrap();
    let canonical_wt = worktree_dir.canonicalize().unwrap();
    conn.execute(
        "INSERT INTO task_claims(task_id, revision, worker_id, worktree, claimed_at, ended) VALUES('021-task', 1, 'worker-1', ?1, 1000, 0)",
        rusqlite::params![canonical_wt.to_str().unwrap()],
    ).unwrap();

    std::fs::write(
        &selected_config,
        "roots: [src]\nresponse:\n  max_output_bytes: 1024\n",
    )
    .unwrap();
    let index_before_auto_rejection = fs::read(&selected_index).unwrap();
    let tasks_db_before_auto_rejection = fs::read(&tasks_db_path).unwrap();
    for (label, invalid_format) in &invalid_formats {
        let mut arguments = json!({});
        arguments["format"] = invalid_format.clone();
        let (wire_bytes, response) = client.call("code_map_overview", arguments);
        let is_error = response["result"]["isError"] == true;
        let error_code = response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if wire_bytes > 1024 || !is_error || error_code != "INVALID_FORMAT" {
            invalid_policy_failures.push(format!(
                "automatic selected root {label}: {wire_bytes} wire bytes, isError={is_error}, code={error_code:?}"
            ));
        }
        if fs::read(&selected_index).ok().as_deref() != Some(index_before_auto_rejection.as_slice()) {
            invalid_policy_failures.push(format!(
                "automatic selected root {label} changed index bytes before rejection"
            ));
        }
        if fs::read(&tasks_db_path).ok().as_deref()
            != Some(tasks_db_before_auto_rejection.as_slice())
        {
            invalid_policy_failures.push(format!(
                "automatic selected root {label} changed task database bytes during rejection"
            ));
        }
        if fs::read(&checkpoint_path).ok() != checkpoint_before {
            invalid_policy_failures.push(format!(
                "automatic selected root {label} changed checkpoint bytes during rejection"
            ));
        }
    }
    assert!(
        invalid_policy_failures.is_empty(),
        "selected-root invalid-format contract failed: {}",
        invalid_policy_failures.join("; ")
    );
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();

    // Call code_map_overview with NO worktree parameter -> automatically resolves to active claim worktree!
    let overview_auto = client.payload(
        "code_map_overview",
        json!({"aspects":["counts","metadata"]}),
    );
    assert_eq!(
        get_workspace_root(&overview_auto),
        canonical_wt.to_str().unwrap()
    );

    // Search with NO worktree parameter -> automatically discovers symbols from the claimed worktree!
    let search_auto = client.payload(
        "code_map_search",
        json!({"pattern": "semantic_worktree_func", "exact": true}),
    );
    assert_eq!(search_auto["nodes"]["total"], 1);
    assert_eq!(
        search_auto["nodes"]["items"][0]["name"],
        "semantic_worktree_func"
    );

    // Multiple claims for the same physical tree are still unambiguous.
    conn.execute(
        "INSERT INTO task_claims VALUES('same-tree', 1, 'worker-2', ?1, 2000, 0)",
        [".worktrees/021-task/../021-task"],
    )
    .unwrap();
    assert_eq!(
        get_workspace_root(&client.payload("code_map_overview", json!({"aspects":["metadata"]}))),
        canonical_wt.to_str().unwrap()
    );

    let other = workspace.path(".worktrees/022-task");
    fs::create_dir_all(other.join("src")).unwrap();
    fs::write(other.join("src/lib.rs"), "pub fn other_tree() {}\n").unwrap();
    conn.execute(
        "INSERT INTO task_claims VALUES('other-tree', 1, 'worker-3', ?1, 3000, 0)",
        [other.to_str().unwrap()],
    )
    .unwrap();
    for arguments in [json!({}), json!({"worktree":"task"})] {
        let (_, response) = client.call("code_map_overview", arguments);
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("WORKTREE_AMBIGUOUS"),
            "{response}"
        );
    }
    assert_eq!(
        get_workspace_root(&client.payload(
            "code_map_overview",
            json!({"worktree":"021-task", "aspects":["metadata"]})
        )),
        canonical_wt.to_str().unwrap()
    );
    assert_eq!(
        get_workspace_root(&client.payload(
            "code_map_overview",
            json!({"worktree":".", "aspects":["metadata"]})
        )),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );
    conn.execute(
        "UPDATE task_claims SET ended = 1 WHERE task_id != '021-task'",
        [],
    )
    .unwrap();

    // 5. When claim is ended (ended = 1) -> automatically falls back to primary root
    conn.execute(
        "UPDATE task_claims SET ended = 1 WHERE task_id = '021-task'",
        [],
    )
    .unwrap();
    let overview_ended = client.payload(
        "code_map_overview",
        json!({"aspects":["counts","metadata"]}),
    );
    assert_eq!(
        get_workspace_root(&overview_ended),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // Changing tasks_db invalidates only cached configuration, never claim results.
    conn.execute(
        "UPDATE task_claims SET ended = 0 WHERE task_id = '021-task'",
        [],
    )
    .unwrap();
    let alternate =
        rusqlite::Connection::open(workspace.path(".forge/alternate-tasks.sqlite")).unwrap();
    alternate
        .execute_batch("CREATE TABLE task_claims(worktree TEXT, ended INTEGER);")
        .unwrap();
    alternate
        .execute(
            "INSERT INTO task_claims VALUES(?1, 0)",
            [other.to_str().unwrap()],
        )
        .unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n",
    );
    assert_eq!(
        get_workspace_root(&client.payload("code_map_overview", json!({"aspects":["metadata"]}))),
        other.canonicalize().unwrap().to_str().unwrap()
    );

    // Missing and empty configurations have the same digest, but only the latter
    // enables the default task database. Observe every transition without restart.
    let primary = workspace.root().canonicalize().unwrap();
    let other = other.canonicalize().unwrap();
    for (configuration, expected) in [
        (None, &primary),
        (Some("{}\n"), &canonical_wt),
        (None, &primary),
        (
            Some("roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n"),
            &other,
        ),
    ] {
        match configuration {
            Some(content) => workspace.write("forge-mcp.yaml", content),
            None => fs::remove_file(workspace.path("forge-mcp.yaml")).unwrap(),
        }
        assert_eq!(
            get_workspace_root(
                &client.payload("code_map_overview", json!({"aspects":["metadata"]}))
            ),
            expected.to_str().unwrap(),
            "configuration {configuration:?}"
        );
    }

    #[cfg(unix)]
    {
        let alias = workspace.path(".forge/alias-tasks.sqlite");
        std::os::unix::fs::symlink("alternate-tasks.sqlite", &alias).unwrap();
        workspace.write(
            "forge-mcp.yaml",
            "roots: [src]\ntasks_db: .forge/alias-tasks.sqlite\n",
        );
        for (target, expected) in [
            ("alternate-tasks.sqlite", other.canonicalize().unwrap()),
            ("tasks.sqlite", canonical_wt.clone()),
            ("missing-tasks.sqlite", primary.clone()),
        ] {
            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(target, &alias).unwrap();
            assert_eq!(
                get_workspace_root(
                    &client.payload("code_map_overview", json!({"aspects":["metadata"]}))
                ),
                expected.to_str().unwrap(),
                "task DB symlink target {target}"
            );
        }
    }

    workspace.write("forge-mcp.yaml", "roots: [src]\ntasks_db: [invalid]\n");
    let (_, malformed) = client.call("code_map_overview", json!({}));
    assert_eq!(malformed["result"]["isError"], true, "{malformed}");
    assert!(
        malformed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("invalid task configuration"),
        "{malformed}"
    );
    // An explicit target does not depend on the primary task-store configuration.
    assert_eq!(
        client.payload(
            "code_map_search",
            json!({
                "pattern":"semantic_worktree_func", "exact":true, "worktree":"021-task"
            })
        )["nodes"]["total"],
        1
    );
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n",
    );
    assert_eq!(
        get_workspace_root(&client.payload("code_map_overview", json!({"aspects":["metadata"]}))),
        other.canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn stdio_server_keeps_its_linked_worktree_and_resolves_registered_sibling_names() {
    let primary = Workspace::new();
    primary.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/tasks.sqlite\n",
    );
    primary.write("src/lib.rs", "pub fn primary_only() {}\n");
    let git = |args: &[&str]| {
        let output = contextunity_forge_mcp::core::git::git_command(primary.root(), args).unwrap();
        assertions::assert_command_succeeded(&output);
    };
    git(&["init", "-q"]);
    git(&["add", "forge-mcp.yaml", "src"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "fixture",
    ]);
    let linked = primary.path(".worktrees/049-current");
    let sibling_container = Workspace::new();
    let sibling = sibling_container.path("051-external");
    git(&["worktree", "add", "--detach", linked.to_str().unwrap()]);
    git(&["worktree", "add", "--detach", sibling.to_str().unwrap()]);
    fs::write(linked.join("src/lib.rs"), "pub fn linked_only() {}\n").unwrap();
    fs::write(sibling.join("src/lib.rs"), "pub fn sibling_only() {}\n").unwrap();
    fs::create_dir_all(primary.path(".forge")).unwrap();
    let conn = rusqlite::Connection::open(primary.path(".forge/tasks.sqlite")).unwrap();
    conn.execute_batch("CREATE TABLE task_claims(worktree TEXT, ended INTEGER);")
        .unwrap();
    for path in [&linked, &sibling] {
        conn.execute(
            "INSERT INTO task_claims VALUES(?1, 0)",
            [path.to_str().unwrap()],
        )
        .unwrap();
    }
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    let custom_db = linked.join(".forge/custom.sqlite");
    command
        .arg("--root")
        .arg(&linked)
        .arg("--db")
        .arg(&custom_db)
        .arg("serve");
    let mut client = Client::with_command(&mut command);
    for worktree in [None, Some(".")] {
        let mut args = json!({"pattern":"linked_only", "exact":true});
        if let Some(worktree) = worktree {
            args["worktree"] = json!(worktree);
        }
        assert_eq!(client.payload("code_map_search", args)["nodes"]["total"], 1);
    }
    assert!(custom_db.is_file());
    assert!(!linked.join(".forge/code-map.sqlite").exists());
    assert_eq!(
        client.payload(
            "code_map_search",
            json!({"pattern":"sibling_only", "exact":true, "worktree":"external"})
        )["nodes"]["total"],
        1
    );
}
