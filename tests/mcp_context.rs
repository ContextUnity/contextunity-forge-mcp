use contextunity_forge_mcp::{
    core::response::{ResponsePolicy, MAX_OUTPUT_BYTES},
    engine::scanner,
    mcp::response,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("forge_mcp_context_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, text: &str) {
        fs::write(self.0.join(path), text).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Client {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    id: usize,
}
impl Client {
    fn new(workspace: &Workspace) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(&workspace.0)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || sender.send(line).is_err() {
                break;
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        let (_, init) = client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"context-test","version":"1"}}));
        assert!(init.get("error").is_none(), "{init}");
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client.input.flush().unwrap();
        client
    }
    fn request(&mut self, method: &str, params: Value) -> (usize, Value) {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP response missing or timed out");
            assert!(
                line.len() <= MAX_OUTPUT_BYTES,
                "JSON-RPC frame was {} bytes",
                line.len()
            );
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == self.id {
                return (line.len(), response);
            }
        }
    }
    fn call(&mut self, name: &str, arguments: Value) -> (usize, Value) {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }
    fn payload(&mut self, name: &str, arguments: Value) -> Value {
        let (_, response) = self.call(name, arguments);
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn wait_for_source_inventory_ttl() {
    std::thread::sleep(Duration::from_millis(5100));
}
#[path = "mcp_context/tasks.rs"]
mod tasks;

#[test]
fn stdio_compact_navigation_preserves_symbol_and_page_contracts() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        "def entry():\n    return target()\ndef target():\n    return 1\n",
    );
    workspace.write("other.py", "def target():\n    return 2\n");
    let mut client = Client::new(&workspace);
    let first = client.payload(
        "code_map_search",
        json!({"pattern":"target","exact":true,"limit":1,"detail":"full"}),
    );
    let page = &first["nodes"];
    assert_eq!(page["has_more"], true);
    let selector = page["items"][0]["inspect_selector"].as_str().unwrap();
    assert_eq!(page["items"][0]["id"], selector);
    assert!(page["items"][0].get("is_test").is_none());
    assert!(page["items"][0].get("generated").is_none());
    let second = client.payload("code_map_search", json!({"pattern":"target","exact":true,"limit":1,"offset":page["next_offset"],"generation":page["generation"]}));
    assert_eq!(second["nodes"]["has_more"], false);
    assert!(second["nodes"].get("next_offset").is_none());
    assert!(second["nodes"].get("continuation_hint").is_none());
    assert_eq!(
        second["freshness"],
        json!({"status":"matched","generation":page["generation"]})
    );
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
    let isolated = client.payload("code_map_explain", json!({"selector":"other.py:target"}));
    assert!(isolated.get("outgoing").is_none());
    let (_, ambiguous) = client.call("code_map_inspect", json!({"selector":"target"}));
    assert_eq!(ambiguous["result"]["isError"], true);
    assert!(ambiguous["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("code_map_inspect({\"selector\":"));
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
    assert_eq!(payload["nodes"]["generation"], "aaaaaaaa");
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
        json!({"target":format!("SELECT '1234567é' AS generation, '0123456789' AS output_root, 'abcdef0123456789' AS corpus_hash, '{details}' AS details")}),
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
    let (_, unknown) = client.call(&"\"\\Привіт".repeat(30_000), json!({}));
    assert!(unknown["result"]["isError"] == true || unknown.get("error").is_some());
    client.payload(
        "session_checkpoint",
        json!({"action":"save","name":"large","content":"\"\\Привіт".repeat(20_000)}),
    );
    for action in ["get", "list"] {
        let (_, response) = client.call(
            "session_checkpoint",
            json!({"action":action,"name":"large"}),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
    }
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
}
#[test]
fn adapter_policy_applies_without_changing_fact_identity_and_rejects_invalid_bounds() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [.]\nresponse:\n  page_size: 2\n  max_output_bytes: 4096\n",
    );
    workspace.write("README.md", "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n");
    let first = scanner::load_adapter(&workspace.0, None).unwrap();
    let mut client = Client::new(&workspace);
    let payload = client.payload("get_doc", json!({"path_or_id":"README.md"}));
    assert_eq!(payload["sections"]["items"].as_array().unwrap().len(), 2);
    let database = workspace.0.join(".forge/code-map.sqlite");
    let before = fs::metadata(&database).unwrap().modified().unwrap();
    workspace.write("forge-mcp.yaml", "roots: [.]\nresponse:\n  detail: full\n  page_size: 3\n  max_output_bytes: 2048\n  source_context:\n    max_body_lines: 10\n");
    let second = scanner::load_adapter(&workspace.0, None).unwrap();
    assert_eq!(first.digest, second.digest);
    let (bytes, response) = client.call("get_doc", json!({"path_or_id":"README.md"}));
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
        "source_context: {max_body_lines: 101}",
        "source_context: {max_body_lines: 0}",
        "detail: enormous",
    ] {
        workspace.write("forge-mcp.yaml", &format!("response:\n  {settings}\n"));
        assert!(
            scanner::load_adapter(&workspace.0, None).is_err(),
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
    let mut client = Client::new(&workspace);
    let initial = client.payload(
        "code_map_search",
        json!({"pattern":"symbol_*","kind":"function"}),
    );
    let generation = initial["nodes"]["generation"].as_str().unwrap();
    assert_eq!(generation.len(), 8);
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
    wait_for_source_inventory_ttl();
    let (_, stale) = client.call(
        "code_map_search",
        json!({"pattern":"symbol_*","offset":30,"generation":initial["nodes"]["generation"]}),
    );
    assert_eq!(stale["result"]["isError"], true);
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
    let response = client.payload(
        "code_map_search",
        json!({"pattern":"target","kind":"function","group_by_file":true}),
    );
    assert_eq!(response["nodes"]["total"], 2);
    assert_eq!(response["nodes"]["items"], json!([]));
    assert_eq!(
        response["nodes"]["grouped_by_file"]["service.py"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
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
    let first = client.payload("get_code_snippet", json!({"selector":"selected"}));
    let text = first["source"].as_str().unwrap();
    assert!(text.contains("Привіт\r\n") || text.contains("Привіт'\r\n"));
    assert!(!text.contains("previous"));
    assert!(!text.contains("following"));
    assert_eq!(first["source_preview"]["body_lines"], 35);
    let second = client.payload("get_code_snippet", json!({"selector":"selected","source_offset":first["source_preview"]["next_source_offset"],"generation":first["generation"]}));
    assert_eq!(second["source_preview"]["body_offset"], 35);
    assert!(!second["source"].as_str().unwrap().contains("def selected"));
    workspace.write(
        "huge.py",
        &format!("def enormous(): return '{}'\n", "Привіт".repeat(50_000)),
    );
    wait_for_source_inventory_ttl();
    let enormous = client.payload("get_code_snippet", json!({"selector":"enormous"}));
    assert_eq!(enormous["source_preview"]["truncated_line"], true);
    assert!(enormous["source_preview"]["next_source_offset"].is_null());
    assert!(enormous["source_preview"]["continuation_hint"]
        .as_str()
        .unwrap()
        .contains("huge.py"));
}
#[cfg(feature = "lang-python")]
#[path = "mcp_context/ast_search.rs"]
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
    let source = "f(1, 2, missing)\nf(1, 1)\nf(1, 2)\n";
    workspace.write("service.py", source);
    wait_for_source_inventory_ttl();
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
        let stored = fs::read(workspace.0.join(".forge/checkpoints.json")).unwrap();
        let persisted: Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(persisted["entries"][name], value);
        for action in ["get", "list"] {
            let (_, response) =
                client.call("session_checkpoint", json!({"action":action,"name":name}));
            assert_eq!(
                response["result"]["isError"], true,
                "oversized {name} {action} created a fabricated page"
            );
            assert!(response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(".forge/checkpoints.json"));
        }
        assert_eq!(
            fs::read(workspace.0.join(".forge/checkpoints.json")).unwrap(),
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
    let result = response::result(
        Ok(
            json!({"nodes":{"total":2,"offset":0,"limit":30,"items":[{"id":"first","text":"\"\\\n".repeat(4000)},{"id":"second","text":"\"\\\n".repeat(4000)}],"has_more":false,"next_offset":null,"generation":"snapshot"}}),
        ),
        &policy,
    );
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);
    assert_ne!(result.is_error, Some(true));
    let wire = serde_json::to_value(result).unwrap();
    let payload: Value =
        serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(payload["nodes"]["next_offset"], 1);
    let result = response::result(Ok(json!({"scalar":"\"\\Привіт".repeat(30_000)})), &policy);
    assert_eq!(result.is_error, Some(true));
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);
}
#[test]
fn stdio_symbol_search_matches_methods_and_functions_interchangeably() {
    let workspace = Workspace::new();
    std::fs::create_dir_all(workspace.0.join("src")).unwrap();
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
