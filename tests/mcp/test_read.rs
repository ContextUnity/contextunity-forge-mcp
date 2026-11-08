//! Contract tests for the layered reader.

use crate::common::Workspace;
use crate::harness::McpTestHarness;
use contextunity_forge_mcp::engine::reader::ReadRequest;
use forge_core::model::ReadMode;
use forge_core::subsystems::{filter_subsystem_tools, SubsystemPreset, SubsystemRegistry};
use serde_json::{json, Value};

fn read_response(client: &mut McpTestHarness, arguments: Value) -> Value {
    client
        .call("forge_read", arguments)
        .expect("MCP transport")
        .1
}

fn read_tool(client: &mut McpTestHarness, arguments: Value) -> Value {
    client
        .payload("forge_read", arguments)
        .expect("MCP tool should succeed")
}

#[test]
fn forge_read_batch_snippet_exports_source() {
    let (w, harness) =
        McpTestHarness::with_files(&[("values.py", "def value():\n    return 42\n")]);
    let mut client = harness.expect("MCP initialize");
    let response = read_tool(
        &mut client,
        json!({"paths":["values.py"], "selector":"value", "output_file":"exports/snippets.txt"}),
    );
    let source = response["batch"][0]["source"].as_str().unwrap();
    let exported = std::fs::read_to_string(w.path("exports/snippets.txt")).unwrap();
    assert_eq!(exported, format!("### values.py\n\n{source}"));
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_validates_ranges_and_argument_names_at_the_mcp_seam() {
    let mut lines_content = String::new();
    for i in 1..=30 {
        lines_content.push_str(&format!("line {i}\n"));
    }
    let mut client = McpTestHarness::indexed(&[
        ("forge-mcp.yaml", "profile: minimal\n"),
        ("notes.txt", lines_content.as_str()),
        ("src/lib.rs", "pub fn value() {}\n"),
        ("test.txt", "hello\n"),
    ])
    .expect("MCP initialize");
    for lines in [
        "",
        "garbage",
        "2-1",
        "0-2",
        "1-x",
        "1-2-3",
        "1..",
        "-2",
        "0",
        "18446744073709551616",
    ] {
        let response = read_response(
            &mut client,
            json!({"path":"notes.txt", "lines":lines, "line_count":1}),
        );
        assert_eq!(response["result"]["isError"], true, "{lines}: {response}");
        assert!(response.to_string().contains("lines"), "{response}");
    }
    for (arguments, expected) in [
        (
            json!({"path":"notes.txt", "start_line":2, "end_line":1}),
            "end_line",
        ),
        (
            json!({"path":"src/lib.rs", "start_line":usize::MAX, "line_count":2}),
            "bounds",
        ),
        (
            json!({"path":"src/lib.rs", "start_line":i64::MAX as usize, "line_count":2}),
            "bounds",
        ),
        (
            json!({"path":"src/lib.rs", "start_line":2, "line_count":usize::MAX}),
            "bounds",
        ),
    ] {
        let details = arguments.to_string();
        let response = read_response(&mut client, arguments);
        assert_eq!(response["result"]["isError"], true, "{details}: {response}");
        assert!(
            response.to_string().contains(expected),
            "{details}: {response}"
        );
    }
    for (arguments, expected) in [
        (
            json!({"path":"notes.txt", "start_line":5, "end_line":10}),
            "5: line 5\n6: line 6\n7: line 7\n8: line 8\n9: line 9\n10: line 10\n",
        ),
        (
            json!({"path":"notes.txt", "lines":"12-15"}),
            "12: line 12\n13: line 13\n14: line 14\n15: line 15\n",
        ),
        (
            json!({"path":"notes.txt", "lines":"20..22"}),
            "20: line 20\n21: line 21\n22: line 22\n",
        ),
        (
            json!({"path":"notes.txt", "start_line":1, "lines":3}),
            "1: line 1\n2: line 2\n3: line 3\n",
        ),
        (
            json!({"path":"notes.txt", "start_line":25, "max_lines":2}),
            "25: line 25\n26: line 26\n",
        ),
    ] {
        let result = read_tool(&mut client, arguments);
        assert_eq!(result["content"], expected, "{result}");
    }

    let rejected = read_response(&mut client, json!({"path":"test.txt", "line_count_max":50}));
    assert_eq!(rejected["result"]["isError"], true);
    let rejected_json: Value =
        serde_json::from_str(rejected["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rejected_json["error"]["code"], "rejected_arguments");
    assert_eq!(rejected_json["error"]["fields"], json!(["line_count_max"]));
    let msg = rejected_json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("Valid supported parameters for 'forge_read'"),
        "expected parameter list in {msg}"
    );
    assert!(
        msg.contains("path") && msg.contains("start_line") && msg.contains("line_count"),
        "expected core params in {msg}"
    );
    assert!(
        msg.contains("Did you mean:"),
        "expected suggestion in {msg}"
    );
    assert!(
        rejected_json["error"]["supported_parameters"].is_array(),
        "expected supported_parameters list"
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_core_modes_and_cli_share_bounded_text_and_exports() {
    use crate::cli::run_cli_json;
    let w = Workspace::with_files(&[
        ("forge-mcp.yaml", "profile: minimal\n"),
        ("notes.txt", "first\r\nПривіт 42\r\nthird\r\nfourth\r\n"),
    ]);
    let mut client = McpTestHarness::new(&w).expect("MCP initialize");
    for (mode, extra, expected) in [
        (
            "lines",
            json!({}),
            "1: first\r\n2: Привіт 42\r\n3: third\r\n4: fourth\r\n",
        ),
        (
            "raw",
            json!({"start_line":2,"line_count":1}),
            "Привіт 42\r\n",
        ),
        (
            "lines",
            json!({"start_line":2,"line_count":1}),
            "2: Привіт 42\r\n",
        ),
        (
            "search",
            json!({"pattern":"Привіт","context":0}),
            "2: Привіт 42\r\n",
        ),
        (
            "search",
            json!({"pattern":"[0-9]+$","regex":true,"context":0}),
            "",
        ),
        (
            "search",
            json!({"pattern":"[0-9]+","regex":true,"context":0}),
            "2: Привіт 42\r\n",
        ),
    ] {
        let mut args = extra;
        args["path"] = json!("notes.txt");
        args["mode"] = json!(mode);
        let result = read_tool(&mut client, args);
        assert_eq!(result["mode"], mode, "{result}");
        assert_eq!(result["content"], expected, "{result}");
        assert!(result.get("Hints").is_none(), "{result}");
    }
    let args = json!({"path":"notes.txt","mode":"raw","start_line":2,"line_count":1,"output_file":"exports/read.txt"});
    let mcp = read_tool(&mut client, args);
    assert_eq!(
        std::fs::read_to_string(w.path("exports/read.txt")).unwrap(),
        "Привіт 42\r\n"
    );
    let output = run_cli_json(
        &w,
        &[
            "read",
            "notes.txt",
            "--mode",
            "raw",
            "--start-line",
            "2",
            "--line-count",
            "1",
            "--output-file",
            "exports/read.txt",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        mcp
    );
    let (_, old) = client
        .call(
            &["get_", "code_snippet"].concat(),
            json!({"selector":"notes.txt"}),
        )
        .expect("MCP transport");
    assert_eq!(old["result"]["isError"], true);
    assert!(!w.db().exists(), "core reads must not create the index");
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_handles_path_aliases_and_rejects_unsafe_paths() {
    let (w, harness) = McpTestHarness::with_files(&[
        (
            "forge-mcp.yaml",
            "profile: minimal\nsensitive: ['secrets/**', '.env*']\n",
        ),
        ("notes.txt", "safe\n"),
        ("small.txt", "inside\n"),
        ("data.txt", "alpha\nbeta\ngamma needle\ndelta\n"),
        ("docs/guide.md", "# Guide\n"),
        ("secrets/token.txt", "secret\n"),
        (".env.local", "secret\n"),
    ]);
    w.write_bytes("binary.dat", b"abc\0def");
    #[cfg(unix)]
    let outside = Workspace::new();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(w.path("notes.txt"), outside.path("alias.txt")).unwrap();
        std::os::unix::fs::symlink(w.root(), outside.path("alias-dir")).unwrap();
    }
    let absolute_data_path = w.root().join("data.txt").display().to_string();
    let parent_absolute = format!(
        "/tmp/../{}/notes.txt",
        w.root().strip_prefix("/").unwrap().display()
    );
    let sibling = format!("{}small.txt", w.root().display());
    let mut client = harness.expect("MCP initialize");

    for (arguments, path, content) in [
        (
            json!({"file_path":"data.txt", "start_line":1, "line_count":2}),
            "data.txt",
            "1: alpha\n2: beta\n",
        ),
        (
            json!({"path":absolute_data_path.clone(), "start_line":1, "line_count":1}),
            "data.txt",
            "1: alpha\n",
        ),
        (
            json!({"file_path":absolute_data_path, "start_line":2, "line_count":1}),
            "data.txt",
            "2: beta\n",
        ),
    ] {
        let result = read_tool(&mut client, arguments);
        assert_eq!(result["path"], path);
        assert_eq!(result["content"], content);
    }
    let search = read_tool(
        &mut client,
        json!({"path":"data.txt", "mode":"search", "query":"needle"}),
    );
    assert!(search["content"].as_str().unwrap().contains("gamma needle"));

    let mut rejected_paths = vec![
        (json!({"path":"../notes.txt"}), Some("relative")),
        (json!({"path":parent_absolute}), Some("relative")),
        (json!({"path":sibling}), None),
        (json!({"path":"/etc/passwd"}), Some("relative")),
        (
            json!({"path":"secrets/token.txt","output_file":"leaked.txt"}),
            Some("SENSITIVE_FILE"),
        ),
        (json!({"path":".env.local"}), Some("SENSITIVE_FILE")),
        (
            json!({"path":"binary.dat","output_file":"leaked.txt"}),
            Some("BINARY_FILE"),
        ),
        (
            json!({"path":"notes.txt","output_file":"../escaped.txt"}),
            Some("relative"),
        ),
        (
            json!({"path":"notes.txt","output_file":".env.export"}),
            Some("SENSITIVE_FILE"),
        ),
        (
            json!({"path":"notes.txt","mode":"snippet"}),
            Some("FEATURE_DISABLED"),
        ),
        (
            json!({"path":"notes.txt","mode":"outline"}),
            Some("FEATURE_DISABLED"),
        ),
        (
            json!({"path":"docs/guide.md","mode":"outline"}),
            Some("FEATURE_DISABLED"),
        ),
        (
            json!({"path":"notes.txt","mode":"search","pattern":"[","regex":true}),
            Some("regex"),
        ),
    ];
    #[cfg(unix)]
    {
        for path in [
            outside.path("alias.txt"),
            outside.path("alias-dir/notes.txt"),
        ] {
            rejected_paths.push((json!({"path":path}), None));
        }
        std::os::unix::fs::symlink(w.path("notes.txt"), w.path("alias.txt")).unwrap();
        rejected_paths.extend([
            (json!({"path":"alias.txt"}), None),
            (json!({"path":"notes.txt","output_file":"alias.txt"}), None),
        ]);
    }
    for (arguments, expected_error) in rejected_paths {
        let response = read_response(&mut client, arguments);
        assert_eq!(response["result"]["isError"], true, "{response}");
        if let Some(expected_error) = expected_error {
            assert!(response.to_string().contains(expected_error), "{response}");
        }
    }
    assert!(!w.path("leaked.txt").exists());
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_delta_never_exposes_sensitive_sibling_through_git_pathspec() {
    use crate::cli::run_cli_json;
    let w = Workspace::with_files(&[(
        "forge-mcp.yaml",
        "profile: minimal\nsensitive: ['secrets/**']\n",
    )]);
    let paths = [
        ":(glob)**",
        "*",
        "?",
        "[ab]",
        ":(literal)secrets/token.txt",
        "--option.txt",
    ];
    for (index, path) in paths.iter().enumerate() {
        w.write(path, &format!("safe-before-{index}\n"));
    }
    w.write("secrets/token.txt", "old-secret\n");
    w.seed_git().expect("seed real Git baseline");
    for (index, path) in paths.iter().enumerate() {
        w.write(path, &format!("safe-after-{index}\n"));
    }
    w.write("secrets/token.txt", "new-secret\n");
    let mut client = McpTestHarness::new(&w).expect("MCP initialize");
    for (index, path) in paths.iter().enumerate() {
        let export = format!("exports/delta-{index}.txt");
        let result = read_tool(
            &mut client,
            json!({"path":path,"mode":"delta","output_file":export}),
        );
        let content = result["content"].as_str().unwrap();
        assert!(
            !content.contains("old-secret") && !content.contains("new-secret"),
            "sensitive sibling leaked: {result}"
        );
        assert!(
            content.contains(&format!("-safe-before-{index}"))
                && content.contains(&format!("+safe-after-{index}")),
            "literal file omitted: {result}"
        );
        assert_eq!(
            content.matches("diff --git ").count(),
            1,
            "delta broadened beyond the admitted file: {content}"
        );
        assert_eq!(std::fs::read_to_string(w.path(&export)).unwrap(), content);
        let output = run_cli_json(
            &w,
            &["read", "--mode", "delta", "--baseline", "HEAD", "--", path],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            cli["content"], result["content"],
            "CLI/MCP delta diverged for {path}"
        );
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_delta_uses_the_admitted_linked_git_context() {
    use crate::cli::run_cli_json;
    let linked = Workspace::with_files(&[]);
    let primary = Workspace::with_files(&[(
        "forge-mcp.yaml",
        &format!(
            "profile: minimal\nsensitive: ['**/secrets/**']\nlinked_workspaces:\n  - name: linked\n    path: {}\n",
            linked.root().display()
        ),
    )]);
    let paths = ["nested/notes.txt", "nested/:(glob)**"];
    for path in paths {
        linked.write(path, "linked-before\n");
        primary.write(format!("[linked]/{path}"), "decoy-before\n");
    }
    linked.write("nested/secrets/token.txt", "secret-before\n");
    primary.seed_git().expect("seed primary Git baseline");
    linked.seed_git().expect("seed linked Git baseline");
    primary
        .create_git_tags(&["primary-only"])
        .expect("tag primary baseline");
    linked
        .create_git_tags(&["linked-only"])
        .expect("tag linked baseline");
    for path in paths {
        linked.write(path, "linked-after\n");
        primary.write(format!("[linked]/{path}"), "decoy-after\n");
    }
    linked.write("nested/secrets/token.txt", "secret-after\n");
    let mut client = McpTestHarness::new(&primary).expect("MCP initialize");
    let mut failures = Vec::new();
    for path in paths {
        let alias = format!("[linked]/{path}");
        let raw = read_tool(&mut client, json!({"path":alias,"mode":"raw"}));
        assert_eq!(raw["content"], "linked-after\n");
        for baseline in [None, Some("linked-only")] {
            let mut request = json!({"path":alias,"mode":"delta"});
            let mut args = vec!["read", "--mode", "delta"];
            if let Some(baseline) = baseline {
                request["baseline"] = json!(baseline);
                args.extend(["--baseline", baseline]);
            }
            args.extend(["--", &alias]);
            let (_, response) = client.call("forge_read", request).expect("MCP transport");
            let output = run_cli_json(&primary, &args);
            let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
            // Exercise both real transports before reporting a runtime-red failure.
            for (transport, value) in [("MCP", response.to_string()), ("CLI", cli.to_string())] {
                if !value.contains("-linked-before")
                    || !value.contains("+linked-after")
                    || value.contains("decoy-")
                    || value.contains("secret-")
                    || value.matches("diff --git ").count() != 1
                {
                    failures.push(format!("{transport} {alias} {baseline:?}: {value}"));
                }
            }
            if !output.status.success() {
                failures.push(format!(
                    "CLI failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    for baseline in ["primary-only", "--output=escaped", "missing-revision"] {
        let (_, response) = client
            .call(
                "forge_read",
                json!({"path":"[linked]/nested/notes.txt","mode":"delta","baseline":baseline}),
            )
            .expect("MCP transport");
        let output = run_cli_json(
            &primary,
            &[
                "read",
                "[linked]/nested/notes.txt",
                "--mode",
                "delta",
                &format!("--baseline={baseline}"),
            ],
        );
        if response["result"]["isError"] != true || output.status.success() {
            failures.push(format!(
                "unavailable/unsafe baseline accepted: {baseline}: {response}"
            ));
        }
    }
    let (_, response) = client
        .call(
            "forge_read",
            json!({"path":"[linked]/nested/secrets/token.txt","mode":"delta"}),
        )
        .expect("MCP transport");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(response.to_string().contains("SENSITIVE_FILE"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_enclosing_hints_resolve_exact_symbols_and_paths() {
    let cases = [("distinct.py", "Second"), ("repeated.py", "First")];
    let mut files: Vec<_> = cases
        .iter()
        .map(|(path, second)| {
            (
                *path,
                format!("class First:\n    def value(self):\n        return 1\n\nclass {second}:\n    def value(self):\n        return 2\n"),
            )
        })
        .collect();
    files.push(("requirements.txt", "django\n".to_owned()));
    files.push((
        "calculator.py",
        "def compute_total(a, b):\n    x = a * 2\n    y = b * 3\n    return x + y\n\nclass TaxCalculator:\n    def calculate_tax(self, amount):\n        return amount * 0.2\n".to_owned(),
    ));
    files.extend([
        ("exact.py", "def exact():\n    return 1\n".to_owned()),
        (
            "nested/exact.py",
            "def shadow():\n    x = 1\n    y = 2\n    return x + y\n".to_owned(),
        ),
    ]);
    let file_refs: Vec<_> = files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    let (_workspace, harness) = McpTestHarness::with_files(&file_refs);
    let mut client = harness.expect("MCP initialize");
    for (path, second) in cases {
        for (line, expected) in [(3, "return 1"), (7, "return 2")] {
            let response = read_tool(
                &mut client,
                json!({"path":path, "start_line":line, "line_count":1}),
            );
            assert_eq!(response["path"], path, "{response}");
            assert_eq!(response["content"], format!("{line}:         {expected}\n"));
            let footer = response["footer"].as_str().unwrap();
            let selector = footer
                .split("selector=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let snippet = read_tool(&mut client, json!({"path":path, "selector":selector}));
            assert_eq!(snippet["node"]["path"], path, "{snippet}");
            let (leading, class) = if line == 3 {
                ("", "First")
            } else {
                ("\n", second)
            };
            assert_eq!(
                snippet["source"],
                format!("{leading}class {class}:\n    def value(self):\n        {expected}\n"),
                "{snippet}"
            );
        }
    }

    let response = read_tool(
        &mut client,
        json!({"path":"calculator.py", "start_line":2, "line_count":2}),
    );
    assert_eq!(response["mode"], "lines");
    let footer = response["footer"]
        .as_str()
        .expect("footer should be present");
    assert!(
        footer.contains("💡 Enclosing symbol: 'fn compute_total'"),
        "unexpected footer: {footer}"
    );
    assert!(
        footer.contains("forge_read(path=\"calculator.py\", selector=\"compute_total\")"),
        "unexpected footer: {footer}"
    );

    let (_, markdown_response) = client
        .call(
            "forge_read",
            json!({"path":"calculator.py", "start_line":2, "line_count":2}),
        )
        .expect("MCP transport");
    let markdown = markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        markdown.contains("💡 Enclosing symbol: 'fn compute_total'"),
        "unexpected markdown: {markdown}"
    );

    let response = read_tool(
        &mut client,
        json!({"path":"calculator.py", "start_line":8, "line_count":1}),
    );
    let footer = response["footer"]
        .as_str()
        .expect("footer should be present for method");
    assert!(
        footer.contains("💡 Enclosing symbol: 'fn calculate_tax'"),
        "unexpected footer: {footer}"
    );
    assert!(
        footer.contains("selector=\"calculate_tax\""),
        "unexpected footer: {footer}"
    );
    for line_count in [1, 3] {
        let response = read_tool(
            &mut client,
            json!({"path":"exact.py", "start_line":2, "line_count":line_count}),
        );
        let footer = response["footer"].as_str().expect("enclosing symbol");
        assert!(footer.contains("fn exact"), "{response}");
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_codemap_hints_and_outline_use_verified_graph_signals() {
    use crate::cli::run_cli_json;
    let (w, harness) = McpTestHarness::indexed_workspace(&[
        (
            "forge-mcp.yaml",
            "profile: minimal\nmodules: {codemap: true}\nroots: [.]\n",
        ),
        (
            "model.py",
            "class Base:\n    pass\n\nclass Child(Base):\n    pass\n\ndef standalone():\n    pass\n\nclass InvalidBase(standalone):\n    pass\n",
        ),
        (
            "service.py",
            "def compute():\n    return 42\n\ndef invoke():\n    return compute()\n",
        ),
        (
            "tests/test_service.py",
            "from service import compute\ndef test_compute():\n    assert compute() == 42\n",
        ),
    ]);
    let mut client = harness.expect("MCP initialize");
    for (arguments, expected) in [
        (json!({"path":"model.py","mode":"outline"}), false),
        (json!({"path":"model.py","mode":"lines"}), false),
        (json!({"path":"model.py","mode":"raw"}), false),
        (
            json!({"path":"model.py","mode":"search","pattern":"class"}),
            false,
        ),
        (json!({"selector":"model.py"}), false),
        (json!({"selector":"Base"}), true),
        (json!({"selector":"Child"}), false),
        (json!({"selector":"standalone"}), false),
    ] {
        let read = read_tool(&mut client, arguments);
        let hints = read["Hints"].as_str().unwrap_or("");
        let implementation = hints
            .split(';')
            .find(|call| call.contains("operation: \"implementations\""));
        assert_eq!(implementation.is_some(), expected, "{read}");
        if let Some(call) = implementation {
            let selector = call
                .split("selector: ")
                .nth(1)
                .unwrap()
                .split(", worktree:")
                .next()
                .unwrap();
            let selector: String = serde_json::from_str(selector).unwrap();
            let followed = client
                .payload(
                    "code_map_query",
                    json!({"operation":"implementations","selector":selector,"worktree":w.root()}),
                )
                .expect("MCP tool should succeed");
            assert_eq!(followed["nodes"]["total"], 1, "{followed}");
        }
    }
    for args in [
        vec!["read", "model.py", "--mode", "outline"],
        vec!["read", "model.py", "--selector", "Base"],
    ] {
        let output = run_cli_json(&w, &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            read["Hints"]
                .as_str()
                .unwrap_or("")
                .contains("operation: \"implementations\""),
            args.contains(&"Base"),
            "{read}"
        );
    }

    let outline = read_tool(&mut client, json!({"path":"service.py","mode":"outline"}));
    assert!(
        outline["content"].as_str().unwrap().contains("compute"),
        "{outline}"
    );
    assert!(
        !outline["content"].as_str().unwrap().contains("return 42"),
        "{outline}"
    );
    let snippet = read_tool(&mut client, json!({"selector":"compute"}));
    assert_eq!(snippet["source"], "def compute():\n    return 42\n");
    let hints = snippet["Hints"].as_str().unwrap();
    assert!(
        hints.contains("code_map_explain(") && hints.contains("code_map_tests("),
        "{snippet}"
    );
    assert!(
        !hints.contains("get_doc") && !hints.contains("task_"),
        "{snippet}"
    );
    assert!(hints.lines().count() <= 3);
    assert!(snippet["graph_pulse"]["callers"].as_u64().unwrap() >= 2);
    assert_eq!(snippet["graph_pulse"]["covering_tests"], 1);
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_docs_outline_keeps_hierarchy_and_filters_disabled_navigation() {
    let mut client = McpTestHarness::indexed(&[
        (
            "forge-mcp.yaml",
            "profile: minimal\nmodules: {docs: true}\nroots: [.]\ndocs: [docs]\n",
        ),
        (
            "docs/guide.md",
            "# Guide\n\nText\n\n## Setup\n\n### Install\n\n```md\n## Code example\n```\n",
        ),
        ("service.py", "def compute():\n    return 42\n"),
    ])
    .expect("MCP initialize");
    let result = read_tool(
        &mut client,
        json!({"path":"docs/guide.md","mode":"outline"}),
    );
    let outline = result["content"].as_str().unwrap();
    assert!(
        outline.contains("# Guide")
            && outline.contains("## Setup")
            && outline.contains("### Install"),
        "{result}"
    );
    assert!(!outline.contains("Code example"), "{result}");
    let hints = result["Hints"].as_str().unwrap();
    assert!(
        hints.contains("get_doc(") && hints.contains("search_docs("),
        "{result}"
    );
    assert!(
        !hints.contains("code_map_") && !hints.contains("task_"),
        "{result}"
    );
    let (_, rejected) = client
        .call("forge_read", json!({"path":"service.py","mode":"outline"}))
        .expect("MCP transport");
    assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    assert!(
        rejected.to_string().contains("FEATURE_DISABLED"),
        "{rejected}"
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn forge_read_acdd_badges_and_hints_follow_the_unique_claimed_worktree() {
    use contextunity_forge_mcp::engine::tasks;
    for with_command in [false, true] {
        let w = Workspace::with_files(&[(
            "forge-mcp.yaml",
            "profile: minimal\nmodules: {acdd: true, blackboard: true}\nroots: [.]\ndocs: [docs]\nmilestones: [docs/milestones]\n",
        )]);
        {
            let profile = forge_tasks::tasks::profile::load_for_workspace(w.root()).unwrap();
            let mut profile = serde_json::to_value(profile.as_ref()).unwrap();
            if let Some(gate) = profile["gates"]
                .as_array_mut()
                .and_then(|arr| arr.iter_mut().find(|g| g["id"] == "contract"))
            {
                gate["command"] = json!(with_command.then_some("reader-check"));
            } else if let Some(tasks) = profile["gates"]
                .as_array_mut()
                .and_then(|arr| {
                    arr.iter_mut()
                        .find_map(|g| g.get_mut("tasks")?.get_mut("gates"))
                })
                .and_then(|g| g.as_array_mut())
            {
                if let Some(gate) = tasks.iter_mut().find(|g| g["id"] == "contract") {
                    gate["command"] = json!(with_command.then_some("reader-check"));
                }
            } else {
                profile["gates"][0]["command"] = json!(with_command.then_some("reader-check"));
            }
            profile["commands"]["reader-check"] =
                json!({"command":"true","output":{"require_counts":false}});
            w.write(
                "reader-profile.yaml",
                &serde_yaml::to_string(&profile).unwrap(),
            );
            let mut config = std::fs::read_to_string(w.path("forge-mcp.yaml")).unwrap();
            config.push_str("acdd_profile: reader-profile.yaml\n");
            w.write("forge-mcp.yaml", &config);
        }
        w.write("allowed.txt", "allowed\n");
        w.write("other.txt", "other\n");
        w.write("docs/milestones/010-reader.md", "---\nid: m-reader\ntitle: Reader\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: read\ntarget: Read scoped text\nproof_policy: seam-test-first\nscope: [allowed.txt]\n```\n");
        let sync = crate::cli::run_cli_json(
            &w,
            &["task", "sync", "--file", "docs/milestones/010-reader.md"],
        );
        assert!(
            sync.status.success(),
            "{}",
            String::from_utf8_lossy(&sync.stderr)
        );
        let task_id = "forge-mcp/m-reader:read";
        tasks::claim(
            w.root(),
            tasks::Claim {
                task_id: task_id.into(),
                stage: "contract".into(),
                worker_id: "reader-contract".into(),
                session_fingerprint: Some("test-session-reader-contract".into()),
                worktree: w.root().to_string_lossy().into_owned(),
                bundle: Some(false),
            },
        )
        .unwrap();
        let mut client = McpTestHarness::new(&w).expect("MCP initialize");
        let allowed = read_tool(&mut client, json!({"path":"allowed.txt"}));
        assert_eq!(allowed["write_scope"], "ALLOWED", "{allowed}");
        let outside = read_tool(&mut client, json!({"path":"other.txt"}));
        assert_eq!(outside["write_scope"], "OUT_OF_SCOPE", "{outside}");
        let hints = outside["Hints"].as_str().unwrap();
        assert!(
            hints.contains("extend_scope") && hints.contains("task_blackboard"),
            "{outside}"
        );
        assert!(
            !hints.contains("code_map_") && !hints.contains("get_doc"),
            "{outside}"
        );
        assert_eq!(hints.contains("verify"), with_command, "{outside}");
        client.finish().expect("server must finish successfully");
    }
}

#[test]
fn minimal_subsystems_filter_tools_and_check_batch_outline_admission() {
    let mut registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    assert_eq!(
        filter_subsystem_tools(["forge_read", "get_doc", "code_map_explain"], &registry),
        ["forge_read"]
    );
    // Minimal has no docs, no codemap
    let req = ReadRequest {
        mode: ReadMode::Outline,
        paths: Some(vec!["README.md".into(), "src/lib.rs".into()]),
        ..Default::default()
    };
    // Should fail because neither subsystem is enabled
    assert!(req
        .requires_index(&forge_core::config::Adapter {
            modules: registry.clone(),
            ..Default::default()
        })
        .is_err());

    // Enable docs only
    registry.enabled.insert("docs".into());
    let adapter_docs = forge_core::config::Adapter {
        modules: registry.clone(),
        ..Default::default()
    };
    // Batch with only markdown paths succeeds with docs enabled
    let req_md = ReadRequest {
        mode: ReadMode::Outline,
        paths: Some(vec!["README.md".into(), "GUIDE.markdown".into()]),
        ..Default::default()
    };
    assert!(req_md.requires_index(&adapter_docs).is_ok());

    // Batch with mixed markdown and code paths must fail because code requires codemap
    let req_mixed = ReadRequest {
        mode: ReadMode::Outline,
        paths: Some(vec!["README.md".into(), "src/lib.rs".into()]),
        ..Default::default()
    };
    assert!(req_mixed.requires_index(&adapter_docs).is_err());

    // Enable codemap as well -> now both docs and codemap are enabled, mixed paths succeed
    registry.enabled.insert("codemap".into());
    let adapter_both = forge_core::config::Adapter {
        modules: registry,
        ..Default::default()
    };
    assert!(req_mixed.requires_index(&adapter_both).is_ok());
}
