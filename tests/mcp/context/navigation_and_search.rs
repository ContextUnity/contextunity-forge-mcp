use super::*;

#[cfg(unix)]
#[test]
fn mcp_navigation_is_independent_of_task_store_special_files() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};

    for kind in ["Fifo", "Directory", "FifoAlias"] {
        let workspace = Workspace::new();
        workspace.write("forge-mcp.yaml", "tasks_db: .forge/state.sqlite\n");
        let target = workspace.path(".forge/state.sqlite");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        match kind {
            "Directory" => std::fs::create_dir(&target).unwrap(),
            "Fifo" | "FifoAlias" => {
                let fifo = workspace.path(".forge/pipe");
                let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
                // SAFETY: CString supplies a live, NUL-terminated path for this
                // synchronous syscall; mkfifo retains no pointer and writes no
                // Rust-owned memory.
                let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
                assert_eq!(
                    result,
                    0,
                    "create task-store FIFO for {kind}: {}",
                    std::io::Error::last_os_error()
                );
                if kind == "FifoAlias" {
                    symlink(&fifo, &target).unwrap();
                } else {
                    std::fs::rename(fifo, &target).unwrap();
                }
            }
            _ => unreachable!(),
        }
        let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");
        let expected_root = workspace.root().canonicalize().unwrap();
        for arguments in [json!({}), json!({"worktree":"."})] {
            let result = client
                .payload("code_map_overview", arguments)
                .expect("MCP tool should succeed");
            assert_eq!(
                result["workspace_root"],
                expected_root.to_str().unwrap(),
                "{kind}: {result}"
            );
        }
        client.finish().expect("server must finish successfully");
    }
}

pub(super) fn query_cli_with_database(
    workspace: &Workspace,
    database: &std::path::Path,
    args: &[&str],
) -> anyhow::Result<Value> {
    let database = database.to_str().expect("database path should be UTF-8");
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["--db", database]);
    full_args.extend_from_slice(args);
    workspace.query(&full_args)
}

#[test]
fn mcp_search_previews_and_impact_depth_defaults_are_bounded() {
    let body_lines = (0..24)
        .map(|line| format!("    value_{line} = {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let long_line = "x".repeat(6000);
    let hit_sources: Vec<_> = (0..4)
        .map(|index| {
            (
                format!("hit_{index}.py"),
                format!("def hit_{index}():\n{body_lines}\n    return \"{long_line}\"\n"),
            )
        })
        .collect();
    let mut files: Vec<(&str, &str)> = hit_sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    files.push((
        "calls.py",
        "def target():\n    return 1\ndef middle():\n    return target()\ndef outer():\n    return middle()\n",
    ));
    let (workspace, harness) = McpTestHarness::indexed_workspace(&files);
    let mut client = harness.expect("MCP initialize");
    let ast_guide = client
        .payload("forge_guide", json!({"topic":"ast"}))
        .expect("MCP tool should succeed");
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
    let (_, tool_list) = client
        .request("tools/list", json!({}))
        .expect("MCP transport");
    let tools = crate::json_array(&tool_list["result"]["tools"]);
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

    let without_previews = client
        .search("hit_*", json!({"kind":"function"}))
        .expect("MCP tool should succeed");
    assert_eq!(without_previews["nodes"]["total"], 4);
    for hit in crate::json_array(&without_previews["nodes"]["items"]) {
        assert!(hit["signature"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(
            hit["inspect_selector"],
            json!({"selector":hit["id"],"show_source":true})
        );
        assert!(hit.get("source").is_none());
    }

    let with_previews = client
        .search(
            "hit_*",
            json!({"kind":"function","include_preview":true,"limit":100}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(with_previews["nodes"]["total"], 4);
    let mut preview_count = 0;
    let mut preview_bytes = 0;
    for hit in crate::json_array(&with_previews["nodes"]["items"]) {
        if let Some(source) = hit["source"].as_str() {
            preview_count += 1;
            preview_bytes += source.len();
            assert!(source.lines().count() <= 20);
            assert!(hit["source_preview"].is_object());
        }
    }
    assert_eq!(preview_count, 3);
    assert!(preview_bytes <= 12 * 1024, "{preview_bytes} preview bytes");

    let impact = client
        .payload("code_map_impact", json!({"selector":"calls.py:target"}))
        .expect("MCP tool should succeed");
    let generic_impact = client
        .payload(
            "code_map_query",
            json!({"operation":"impact","selector":"calls.py:target"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(impact["depth"], 1);
    assert_eq!(generic_impact["depth"], 2);
    let names = |value: &Value| {
        crate::json_array(&value["nodes"]["items"])
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    assert!(!names(&impact).iter().any(|name| name == "outer"));
    assert!(names(&generic_impact).iter().any(|name| name == "outer"));

    let db = workspace.path(".forge/cli.sqlite");
    let run_cli = |args: &[&str]| query_cli_with_database(&workspace, &db, args);
    let cli_impact = run_cli(&["query", "impact", "calls.py:target"]).expect("CLI should succeed");
    let cli_generic_impact =
        run_cli(&["query", "run", "impact", "calls.py:target"]).expect("CLI should succeed");
    assert_eq!(cli_impact["depth"], 1);
    assert_eq!(cli_generic_impact["depth"], 2);
    assert!(!names(&cli_impact).iter().any(|name| name == "outer"));
    assert!(names(&cli_generic_impact)
        .iter()
        .any(|name| name == "outer"));
    client.finish().expect("server must finish successfully");
}

#[test]
fn cli_selector_commands_apply_workspace_path_scope() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[
        ("service.py", "def target():\n    return 1\n"),
        ("other.py", "def target():\n    return 2\n"),
    ]);

    let forge_dir = workspace.path(".forge");
    let db = forge_dir.join("cli.sqlite");
    let run_cli = |args: &[&str]| query_cli_with_database(&workspace, &db, args);
    let mut mcp = harness.expect("MCP initialize");

    let inspected =
        run_cli(&["query", "inspect", "target", "--path", "other.py"]).expect("CLI should succeed");
    assert_eq!(inspected["node"]["path"], "other.py");
    let explained =
        run_cli(&["query", "explain", "target", "--path", "other.py"]).expect("CLI should succeed");
    assert_eq!(explained["node"]["path"], "other.py");
    let impacted = run_cli(&[
        "query", "impact", "target", "--depth", "0", "--path", "other.py",
    ])
    .expect("CLI should succeed");
    assert_eq!(impacted["selector"]["path"], "other.py");
    let tested =
        run_cli(&["query", "tests", "target", "--path", "other.py"]).expect("CLI should succeed");
    assert_eq!(tested["selector"]["path"], "other.py");
    let removed =
        run_cli(&["query", "remove", "target", "--path", "other.py"]).expect("CLI should succeed");
    assert_eq!(removed["selected_ids"]["total"], 1);

    let mcp_scoped = mcp
        .inspect("target", json!({"path":"other.py","format":"json"}))
        .expect("MCP tool should succeed");
    assert_eq!(mcp_scoped["node"]["path"], "other.py");
    assert_eq!(inspected["node"]["line"], mcp_scoped["node"]["line"]);

    let invalid_cli_format = crate::cli::run_cli(
        &workspace,
        &["--format", "plain", "query", "inspect", "target"],
    );
    assert!(
        !invalid_cli_format.status.success(),
        "CLI accepted unsupported format `plain`"
    );
    let (_, invalid_mcp_format) = mcp
        .call(
            "code_map_inspect",
            json!({"selector":"target","format":"plain"}),
        )
        .expect("MCP transport");
    assert_eq!(invalid_mcp_format["result"]["isError"], true);
    let invalid_mcp_error: Value = serde_json::from_str(
        invalid_mcp_format["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(invalid_mcp_error["error"]["code"], "INVALID_FORMAT");
    mcp.finish().expect("server must finish successfully");
}

#[test]
fn compact_page_metadata_preserves_pagination_evidence() {
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
    assert_eq!(crate::json_array(&payload["nodes"]["items"]).len(), 30);
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

#[test]
fn mcp_zero_match_search_has_compact_page() {
    let mut client =
        McpTestHarness::indexed(&[("service.py", "def useful_symbol():\n    return 1\n")])
            .expect("MCP initialize");
    let query = json!({"pattern":"*noise*","path":"service.py"});
    let payload = client
        .payload("code_map_search", query)
        .expect("MCP tool should succeed");
    assert_eq!(
        payload,
        json!({"pattern":"*noise*","nodes":{"total":0,"items":[]}})
    );
    client.finish().expect("server must finish successfully");
}

#[cfg(feature = "lang-rust")]
#[test]
fn mcp_distinguishes_known_external_imports_from_missing_sources() {
    let (_workspace, harness) =
        McpTestHarness::with_files(&[("main.rs", "use std::fmt::Debug;\nuse missing::Thing;\n")]);
    let mut client = harness.expect("MCP initialize");
    let overview = client
        .payload("code_map_overview", json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(overview["counts"]["external_imports"], 2);
    assert_eq!(overview["counts"]["unresolved"], 0);
    let analysis = client
        .payload("code_map_analyze", json!({"target":"main.rs"}))
        .expect("MCP tool should succeed");
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
    assert!(crate::json_array(&analysis["resolution"]["items"]).is_empty());
    client.finish().expect("server must finish successfully");
}

#[cfg(feature = "lang-python")]
#[test]
fn mcp_symbol_search_groups_the_current_page_by_file() {
    let mut client = McpTestHarness::indexed(&[(
        "service.py",
        "def target(): return 1\ndef target_helper(): return target()\n",
    )])
    .expect("MCP initialize");
    for (pattern, expected) in [("target", 1), ("target*", 2)] {
        let response = client
            .payload(
                "code_map_search",
                json!({"pattern":pattern,"kind":"function","group_by_file":true}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(response["nodes"]["total"], expected, "{pattern}");
        assert_eq!(response["nodes"]["items"], json!([]));
        assert_eq!(
            crate::json_array(&response["nodes"]["grouped_by_file"]["service.py"]).len(),
            expected,
            "{pattern}"
        );
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn mcp_symbol_search_matches_methods_and_functions_interchangeably() {
    let mut client = McpTestHarness::indexed(&[(
        "src/lib.rs",
        "pub struct Server;\nimpl Server {\n    pub fn admit(&self) {}\n}\n",
    )])
    .expect("MCP initialize");
    let result = client
        .search("admit*", json!({"kind":"method","path":"src/lib.rs"}))
        .expect("MCP tool should succeed");
    assert_eq!(result["nodes"]["total"], 1);
    assert_eq!(result["nodes"]["items"][0]["name"], "admit");
    let result_fn = client
        .search("admit*", json!({"kind":"function","path":"src/lib.rs"}))
        .expect("MCP tool should succeed");
    assert_eq!(result_fn["nodes"]["total"], 1);
    assert_eq!(result_fn["nodes"]["items"][0]["name"], "admit");
    client.finish().expect("server must finish successfully");
}
