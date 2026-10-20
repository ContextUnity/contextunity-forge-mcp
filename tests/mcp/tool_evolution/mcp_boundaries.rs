use super::*;

#[test]
fn ergonomic_search_parameters_preserve_scope_and_paging() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("src/app.py", "def first(): pass\ndef second(): pass\n");
    w.write("other.py", "def outside(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);
    let canonical = m.ok("code_map_search", json!({"pattern":"first"}));
    for args in [
        json!({"query":"first"}),
        json!({"pattern":"first","path":""}),
        json!({"query":"first","path":""}),
    ] {
        let result = m.ok("code_map_search", args);
        assert_eq!(result["nodes"], canonical["nodes"]);
    }
    for args in [
        json!({"pattern":"*","path":"src"}),
        json!({"query":"*","path":"src/app.py"}),
        json!({"pattern":"*","path":"src","exact":true}),
    ] {
        let scope = args["path"].as_str().unwrap().to_owned();
        let result = m.ok("code_map_search", args);
        let hits = result["nodes"]["items"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| {
            let path = hit["path"].as_str().unwrap();
            path == scope || path.starts_with(&format!("{scope}/"))
        }));
        assert!(hits.iter().any(|hit| hit["name"] == "first"));
        assert!(hits.iter().any(|hit| hit["name"] == "second"));
    }
    let all_functions = m.ok(
        "code_map_search",
        json!({"pattern":"*","kind":"function","path":""}),
    );
    assert_eq!(all_functions["nodes"]["total"], 3);
    let first = m.ok(
        "code_map_search",
        json!({"pattern":"*","path":"src","kind":"function","limit":1}),
    );
    assert_eq!(first["nodes"]["total"], 2);
    let next = m.ok("code_map_search", json!({"pattern":"*","path":"src","kind":"function","limit":1,"offset":first["nodes"]["next_offset"],"generation":first["nodes"]["generation"]}));
    assert_eq!(next["nodes"]["total"], 2);
    assert_ne!(
        first["nodes"]["items"][0]["id"],
        next["nodes"]["items"][0]["id"]
    );
    for args in [
        json!({"pattern":"*"}),
        json!({"query":"*","path":""}),
        json!({"pattern":"*","kind":""}),
        json!({"pattern":"***","path":"src"}),
    ] {
        m.err("code_map_search", args, "pattern must contain");
    }
    for path in ["../src", "/src", "src//app.py"] {
        m.err(
            "code_map_search",
            json!({"pattern":"*","path":path}),
            "workspace-relative",
        );
    }
}

#[test]
fn ergonomic_source_budgets_are_clamped_at_mcp_boundary() {
    use serde_json::json;
    let w = Workspace::new();
    let source = format!(
        "{}def preview():\n{}",
        "# context\n".repeat(30),
        "    x=1\n".repeat(510)
    );
    assert!(source.len() < 8 * 1024);
    w.write("app.py", &source);
    w.build();
    let mut m = Mcp::new(&w);
    for tool in ["code_map_inspect", "code_map_explain", "get_code_snippet"] {
        let clamped = m.ok(tool, json!({"selector":"app.py:preview","show_source":true,"leading_lines":30,"max_body_lines":501}));
        let bounded = m.ok(tool, json!({"selector":"app.py:preview","show_source":true,"leading_lines":20,"max_body_lines":500}));
        assert_eq!(clamped["source"], bounded["source"], "{tool}");
        assert_eq!(
            clamped["source_preview"], bounded["source_preview"],
            "{tool}"
        );
        assert_eq!(clamped["source_preview"]["body_lines"], 500);
        assert_eq!(clamped["source_preview"]["leading_lines"], 20);
        assert_eq!(clamped["source_preview"]["has_more"], true);
        let clamped_low = m.ok(
            tool,
            json!({"selector":"app.py:preview","show_source":true,"max_body_lines":0}),
        );
        assert_eq!(clamped_low["source_preview"]["body_lines"], 1, "{tool}");
    }
    let first_page = m.ok(
        "get_code_snippet",
        json!({"selector":"app.py:preview","show_source":true,"leading_lines":20,"max_body_lines":501}),
    );
    let next_page = m.ok(
        "get_code_snippet",
        json!({"selector":"app.py:preview","show_source":true,"leading_lines":20,"max_body_lines":500,"source_offset":first_page["source_preview"]["next_source_offset"],"generation":first_page["generation"]}),
    );
    assert_eq!(next_page["source_preview"]["body_offset"], 500);
    assert_eq!(next_page["source_preview"]["body_lines"], 11);
    assert_eq!(next_page["source_preview"]["has_more"], false);
}

#[test]
fn ergonomic_task_aliases_reach_existing_operations() {
    use serde_json::json;
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "roots: [.]\ndocs: [docs]\nmilestones: [docs/milestones]\n",
    );
    w.write("app.py", "def first(): pass\n");
    w.write("docs/milestones/010-aliases.md", "---\nid: m-aliases\ntitle: Parameter aliases\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: aliases\ntarget: Accept parameter aliases\nproof_policy: seam-test-first\nscope: [app.py]\nsubtasks:\n  - subtask_ref: parameters\n    title: Accept alternate parameter names\n```\n");
    w.build();
    w.query(&["task", "sync", "--file", "docs/milestones/010-aliases.md"]);
    let task_id = "forge-mcp/m-aliases:aliases";
    let inspected = w.query(&["task", "inspect", task_id]);
    for args in [
        vec!["task", "show", task_id],
        vec!["task", "inspect", "--task", task_id],
        vec!["task", "show", "--task", task_id],
    ] {
        assert_eq!(w.query(&args), inspected);
    }
    let compact = w.query(&["task", "list", "--milestone", "010", "--detail", "compact"]);
    assert_eq!(compact["tasks"].as_array().unwrap().len(), 1);
    let full = w.query(&["task", "list", "--milestone", "010", "--full"]);
    assert_eq!(
        w.query(&["task", "list", "--milestone", "010", "--detail", "full"]),
        full
    );
    let mut m = Mcp::new(&w);
    let canonical = m.ok("task_list", json!({"milestone_ref":"010","detail":"full"}));
    assert_eq!(
        m.ok("task_list", json!({"milestone":"010","detail":"full"})),
        canonical
    );
    assert_eq!(canonical["tasks"].as_array().unwrap().len(), 1);
    let positional = run_cli(
        &w,
        &[
            "task",
            "submit",
            task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            "{}",
        ],
    );
    let flagged = run_cli(
        &w,
        &[
            "task",
            "submit",
            "--task",
            task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            "{}",
        ],
    );
    assert!(!positional.status.success());
    assert_eq!(flagged.status.code(), positional.status.code());
    assert_eq!(flagged.stderr, positional.stderr);
    for args in [
        vec!["task", "inspect"],
        vec!["task", "inspect", task_id, "--task", task_id],
        vec![
            "task",
            "submit",
            task_id,
            "--task",
            task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            "{}",
        ],
        vec![
            "task",
            "sync",
            "010",
            "--file",
            "docs/milestones/010-aliases.md",
        ],
        vec!["task", "list", "--detail", "verbose"],
        vec!["task", "list", "--full", "--detail", "compact"],
    ] {
        assert_eq!(run_cli(&w, &args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn symbol_boundary_recovery_and_search_preview_resilience() {
    use contextunity_forge_mcp::core::response::{QueryOptions, ResponsePolicy, SourceOptions};
    use serde_json::json;
    let cases = [
        ("alias.py", "from typing import Literal\nStrategy = Literal['direct', 'fallback']; neighbor = 1\n", "Strategy", "type", "Strategy = Literal['direct', 'fallback']"),
        ("variable.py", "pool_size = 3; neighbor = 1\n", "pool_size", "variable", "pool_size = 3"),
        ("fields.py", "class Pool:\n    size = (\n        3\n    )\n    def __init__(self):\n        self.strategy = 'direct'\n", "size", "field", "    size = (\n        3\n    )\n"),
        ("instance.py", "class Pool:\n    def __init__(self):\n        self.strategy = 'direct'\n", "strategy", "field", "        self.strategy = 'direct'\n"),
    ];
    for (path, text, name, kind, expected) in
        cases
            .into_iter()
            .chain(cfg!(feature = "lang-html").then_some((
                "template.html",
                "<main>\n{% set strategy = 'direct' %}\n</main>\n",
                "strategy",
                "variable",
                "{% set strategy = 'direct' %}\n",
            )))
    {
        let w = Workspace::new();
        w.write(path, text);
        w.build();
        let conn = w.open();
        let rows = reader::rows(
            &conn,
            "SELECT id,line,end_line FROM nodes WHERE name=?1 AND kind=?2",
            &[&name, &kind],
            10,
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "{path}: {rows:?}");
        let id = rows[0]["id"].as_str().unwrap();
        let page = QueryOptions::resolve(&ResponsePolicy::default(), None, 0, None, None).unwrap();
        let source = SourceOptions {
            enabled: true,
            leading_lines: 0,
            max_body_lines: 35,
            offset: 0,
        };
        let snippet =
            contextunity_forge_mcp::db::symbols::snippet_paged(&conn, w.root(), id, &source, &page)
                .unwrap();
        assert_eq!(snippet["source"], expected, "{path}: {snippet}");
        let mut m = Mcp::new(&w);
        for tool in ["code_map_inspect", "code_map_explain", "get_code_snippet"] {
            let result = m.ok(
                tool,
                json!({"selector":id,"show_source":true,"leading_lines":0}),
            );
            assert_eq!(result["source"], expected, "{path}: {tool}: {result}");
            assert_eq!(result["source_preview"]["start_line"], rows[0]["line"]);
            assert_eq!(result["source_preview"]["end_line"], rows[0]["end_line"]);
        }
        let result = m.ok("code_map_search", json!({"pattern":name,"exact":true}));
        let hit = &result["nodes"]["items"][0];
        assert_eq!(hit["id"], id);
        assert!(hit["source"].as_str().unwrap().contains(expected));
    }

    // Warm the real server's read snapshot before changing source so these calls
    // exercise per-hit preview verification rather than index refresh.
    for failure in ["boundary", "digest", "missing"] {
        let w = Workspace::new();
        w.write("a.py", "def resilient_hit(): return 1\n# no declaration\n");
        w.write("z.py", "def resilient_hit(): return 2\n");
        w.build();
        if failure == "boundary" {
            let conn = rusqlite::Connection::open(w.db()).unwrap();
            conn.execute("UPDATE nodes SET line=2,end_line=2 WHERE name='resilient_hit' AND path_id=(SELECT path_id FROM path_dictionary WHERE path='a.py')", []).unwrap();
            contextunity_forge_mcp::core::commitments::seal(&conn).unwrap();
        }
        let mut m = Mcp::new(&w);
        m.ok("code_map_overview", json!({}));
        match failure {
            "digest" => w.write("a.py", "def resilient_hit(): return 9\n"),
            "missing" => fs::remove_file(w.path("a.py")).unwrap(),
            _ => {}
        }
        for include_preview in [false, true] {
            let result = m.ok(
                "code_map_search",
                json!({"pattern":"resilient_hit","exact":true,"include_preview":include_preview}),
            );
            assert_eq!(result["nodes"]["total"], 2, "{failure}: {result}");
            let hits = result["nodes"]["items"].as_array().unwrap();
            assert_eq!(hits.len(), 2);
            for (hit, path) in hits.iter().zip(["a.py", "z.py"]) {
                assert_eq!(hit["path"], path, "{failure}: {result}");
                assert_eq!(
                    hit["line"],
                    if failure == "boundary" && path == "a.py" {
                        2
                    } else {
                        1
                    }
                );
                assert_eq!(hit["inspect_selector"]["selector"], hit["id"]);
                assert_eq!(hit["inspect_selector"]["show_source"], true);
            }
            assert!(
                hits[0].get("source_preview").is_none(),
                "{failure}: {result}"
            );
            assert!(hits[1]["source_preview"].is_object(), "{failure}: {result}");
        }
        if failure == "boundary" {
            for tool in ["code_map_inspect", "code_map_explain", "get_code_snippet"] {
                m.err(
                    tool,
                    json!({"selector":"a.py:resilient_hit","show_source":true}),
                    "indexed symbol boundary is unavailable",
                );
            }
        }
    }
}

#[test]
fn mcp_selector_and_source_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("scanner.py", "def scanner():\n    return 42\n");
    w.write(
        "src/app.py",
        "# license\ndef first(): pass\ndef last(): pass\n",
    );
    w.write("comments.py", "# license only\n# no code body\n");
    w.write("docs/contract.md", "# Contract\n\nA documented contract.\n");
    w.build();
    let mut m = Mcp::new(&w);

    for tool in [
        "code_map_inspect",
        "code_map_explain",
        "code_map_impact",
        "code_map_tests",
        "code_map_prove_removal",
        "get_code_snippet",
    ] {
        m.err(tool, json!({"selector":"scanner"}), "ambiguous selector");
        m.err(tool, json!({"selector":""}), "selector is empty");
        m.err(tool, json!({"selector":"   "}), "selector is empty");
        m.err(
            tool,
            json!({"selector":"docs/contract.md"}),
            "get_doc or search_docs",
        );
    }
    for selector in [
        "scanner.py:scanner",
        "scanner.py::scanner",
        "scanner.py:1",
        "scanner.py#L1",
    ] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "function", "{selector}: {result}");
    }
    for selector in ["file:src/app.py", "file://src/app.py", "./src/app.py"] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "module", "{selector}: {result}");
    }
    for selector in [":::", "#Labc", "nonexistent/path.py:9999"] {
        m.err(
            "code_map_inspect",
            json!({"selector":selector}),
            "selector not found",
        );
    }
    m.err(
        "code_map_inspect",
        json!({"selector":"%.py:scanner"}),
        "selector not found",
    );
    let first = m.ok(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","leading_lines":0}),
    );
    assert_eq!(first["source"], "def first(): pass\n");
    let last = m.ok(
        "get_code_snippet",
        json!({"selector":"src/app.py:last","leading_lines":0}),
    );
    assert_eq!(last["source"], "def last(): pass\n");
    let one_line = m.ok(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","leading_lines":0,"max_body_lines":0}),
    );
    assert_eq!(one_line["source"], "def first(): pass\n");
    assert_eq!(one_line["source_preview"]["body_lines"], 1);
    assert_eq!(one_line["source_preview"]["has_more"], false);
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","leading_lines":-1}),
        "expected usize",
    );
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","max_body_lines":-1}),
        "expected usize",
    );
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","source_offset":-1}),
        "expected usize",
    );
    let comments = m.ok("get_code_snippet", json!({"selector":"comments.py"}));
    assert_eq!(comments["node"]["kind"], "module");
    assert_eq!(comments["source"], "# license only\n# no code body\n");
}

#[test]
fn mcp_graph_depth_cycles_and_direction_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write(
        "tests/test_cycle.py",
        "from cycle import a\ndef test_a(): a()\n",
    );
    w.build();
    let mut m = Mcp::new(&w);
    for depth in [0, 1, 16] {
        let result = m.ok(
            "code_map_impact",
            json!({"selector":"cycle.py:a","depth":depth}),
        );
        assert_eq!(result["depth"], depth);
        assert!(result["nodes"]["total"].as_u64().unwrap() >= 1);
    }
    m.err(
        "code_map_impact",
        json!({"selector":"cycle.py:a","depth":17}),
        "depth must be <=16",
    );
    let inbound = m.ok(
        "code_map_tests",
        json!({"selector":"cycle.py:a","direction":"inbound"}),
    );
    assert!(inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "test_a"));
    let test = inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == "test_a")
        .unwrap();
    assert_eq!(test["connection"]["relation"], "direct");
    assert_eq!(test["connection"]["edge_kind"], "calls");
    let outbound = m.ok(
        "code_map_tests",
        json!({"selector":"tests/test_cycle.py:test_a","direction":"outbound"}),
    );
    assert!(outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "a"));
    m.err(
        "code_map_tests",
        json!({"selector":"cycle.py:a","direction":"sideways"}),
        "direction must be inbound or outbound",
    );
    let explained = m.ok("code_map_explain", json!({"selector":"cycle.py:a"}));
    assert_eq!(explained["node"]["kind"], "function");
    let removal = m.ok("code_map_prove_removal", json!({"selector":"cycle.py:a"}));
    assert_eq!(removal["safe_to_remove"], false);
}

#[test]
fn mcp_search_ast_and_document_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write(
        "src/app.py",
        "from typing import Literal\nModelPoolStrategy = Literal[\"direct\", \"fallback\"]\ndef item_name(): pass\ndef itemXname(): pass\nprint(item_name())\n",
    );
    w.write(
        "docs/intro.md",
        "---\ndoc_type: guide\ntitle: Intro\n---\n# Intro\n\nA searchable contract.\n",
    );
    w.build();
    let mut m = Mcp::new(&w);
    let overview = m.ok("code_map_overview", json!({"limit":1}));
    assert!(overview["generation"].is_string());
    for pattern in ["*", "***", "_", "%", "\\", "'", "\""] {
        m.err(
            "code_map_search",
            json!({"pattern":pattern}),
            "pattern must contain",
        );
    }
    let prefix = m.ok(
        "code_map_search",
        json!({"pattern":"item*","kind":"function","limit":1}),
    );
    assert_eq!(prefix["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(prefix["nodes"]["total"], 2);
    let scoped = m.ok(
        "code_map_search",
        json!({"pattern":"item*","kind":"function","path":"src/app.py"}),
    );
    assert_eq!(scoped["nodes"]["total"], 2);
    let outside = m.ok("code_map_search", json!({"pattern":"item*","path":"docs"}));
    assert_eq!(outside["nodes"]["total"], 0);
    m.err(
        "code_map_search",
        json!({"pattern":"item*","path":"../src"}),
        "workspace-relative",
    );
    let literal_underscore = m.ok("code_map_search", json!({"pattern":"item_*"}));
    assert_eq!(literal_underscore["nodes"]["total"], 1);
    for pattern in ["item%*", "item\\*", "item'*", "item\"*"] {
        let result = m.ok("code_map_search", json!({"pattern":pattern}));
        assert_eq!(result["nodes"]["total"], 0, "{pattern}: {result}");
    }

    let type_search = m.ok("code_map_search", json!({"pattern":"ModelPoolStrategy"}));
    assert_eq!(type_search["nodes"]["total"], 1);
    assert!(type_search["nodes"]["items"][0]["source_preview"].is_object());

    let ast = m.ok(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","path":"src/app.py"}),
    );
    assert!(!ast["matches"]["items"].as_array().unwrap().is_empty());

    let ranked = m.ok("code_map_search", json!({"pattern":"item_name","limit":10}));
    let ranked_total = ranked["nodes"]["total"].clone();
    assert!(ranked_total.as_u64().unwrap() > 0);
    let empty_ranked_page = m.ok(
        "code_map_search",
        json!({"pattern":"item_name","limit":10,"offset":100,"generation":ranked["nodes"]["generation"]}),
    );
    assert!(empty_ranked_page["nodes"]["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(empty_ranked_page["nodes"]["total"], ranked_total);
    m.err(
        "ast_grep_search",
        json!({"pattern":"def (","language":"python"}),
        "pattern",
    );
    m.err(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"unknown"}),
        "unknown",
    );
    m.err(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","path":""}),
        "path",
    );

    let docs = m.ok("search_docs", json!({"query":"searchable"}));
    assert!(docs["sections"]["total"].as_u64().unwrap() >= 1);
    assert!(docs["sections"]["items"][0].get("excerpt").is_none());
    let excerpt = m.ok(
        "search_docs",
        json!({"query":"searchable","include_excerpt":true}),
    );
    assert!(excerpt["sections"]["items"][0]["excerpt"]
        .as_str()
        .unwrap()
        .contains("[searchable]"));
    m.err("search_docs", json!({"query":""}), "search query is empty");
    let quoted = m.ok("search_docs", json!({"query":"\""}));
    assert_eq!(quoted["sections"]["total"], 0);
    let filtered = m.ok(
        "search_docs",
        json!({"query":"searchable","doc_type":"no-such-type"}),
    );
    assert_eq!(filtered["sections"]["total"], 0);
    let doc = m.ok("get_doc", json!({"path_or_id":"docs/intro.md"}));
    assert!(doc["sections"]["total"].as_u64().unwrap() >= 1);
    m.err(
        "get_doc",
        json!({"path_or_id":"docs/intro.md","section":"NonExistentSection"}),
        "document or section not found",
    );
}

#[test]
fn ast_language_inference_and_actionable_diagnostics() {
    use contextunity_forge_mcp::engine::languages;
    use serde_json::json;
    let w = Workspace::new();
    w.write(
        "app.py",
        "class Widget:\n    pass\ndef target(value):\n    return value\n",
    );
    w.write("lib.rs", "fn target() {}\n");
    w.build();
    let mut m = Mcp::new(&w);
    for (path, pattern) in [("app.py", "class Widget"), ("app.py", "def target($$$)")]
        .into_iter()
        .chain(cfg!(feature = "lang-rust").then_some(("lib.rs", "fn target")))
    {
        let result = m.ok("ast_grep_search", json!({"pattern":pattern,"path":path}));
        assert_eq!(result["matches"]["total"], 1, "{pattern}: {result}");
    }
    // Explicit language supports a workspace scope without a file extension.
    let explicit = m.ok(
        "ast_grep_search",
        json!({"pattern":"class Widget","language":"python"}),
    );
    assert_eq!(explicit["matches"]["total"], 1);
    for language in [json!(null), json!("")] {
        let inferred = m.ok(
            "ast_grep_search",
            json!({"pattern":"class Widget","language":language,"path":"app.py"}),
        );
        assert_eq!(inferred["matches"]["total"], 1);
    }
    for path in [None, Some(""), Some("src"), Some("unknown.xyz")] {
        let (_, response) = m.call(
            "ast_grep_search",
            json!({"pattern":"class Widget","path":path}),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        for profile in languages::profiles() {
            assert!(message.contains(profile.id()), "{message}");
        }
        assert!(
            message.contains("language") && message.contains("path"),
            "{message}"
        );
        assert!(message.contains("forge_guide"), "{message}");
        assert!(message.contains("Examples:"), "{message}");
    }
    for language in ["python"]
        .into_iter()
        .chain(cfg!(feature = "lang-rust").then_some("rust"))
    {
        let (_, response) = m.call(
            "ast_grep_search",
            json!({"pattern":"def (","language":language}),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains("unsupported_ast_pattern"), "{message}");
        assert!(message.contains(" at 1:"), "{message}");
        assert!(
            message.contains("Hint:") && message.contains("Examples"),
            "{message}"
        );
        for example in languages::require(language)
            .unwrap()
            .ast_search_capabilities()
            .valid_examples
        {
            assert!(
                message.contains(&example.replace('\n', "\n    ")),
                "{message}"
            );
        }
        assert!(!message.contains("\"diagnostic\":"), "{message}");
    }
}

#[test]
fn ast_diagnostics_reject_invalid_patterns_without_language_candidates() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("README.md", "# No Python sources\n");
    w.build();
    let mut m = Mcp::new(&w);
    let (_, response) = m.call(
        "ast_grep_search",
        json!({"pattern":"def (","language":"python"}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    let message = response["result"]["content"][0]["text"].as_str().unwrap();
    for expected in ["unsupported_ast_pattern", "python", "Hint:", "Examples"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn ast_grep_empty_symbol_prefilter_preserves_local_matches() {
    use serde_json::json;
    let w = Workspace::new();
    for path in ["src/a.py", "src/b.py", "src_other.py"] {
        w.write(
            path,
            "def compute():\n    local_value = 1 + 2\n    return local_value\n",
        );
    }
    w.build();
    let conn = w.open();
    let indexed: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE name='local_value'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 0);
    let mut m = Mcp::new(&w);
    for (path, expected) in [
        (None, vec!["src/a.py", "src/b.py", "src_other.py"]),
        (Some("src"), vec!["src/a.py", "src/b.py"]),
        (Some("src/a.py"), vec!["src/a.py"]),
    ] {
        let result = m.ok(
            "ast_grep_search",
            json!({"pattern":"local_value = $EXPR","language":"python","path":path}),
        );
        assert_eq!(result["matches"]["total"], expected.len(), "{result}");
        let items = result["matches"]["items"].as_array().unwrap();
        let paths: Vec<_> = items
            .iter()
            .map(|item| item["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, expected, "{result}");
        assert!(items.iter().all(|item| item["line"] == 2), "{result}");
    }
    let first = m.ok(
        "ast_grep_search",
        json!({"pattern":"local_value = $EXPR","language":"python","path":"src","limit":1}),
    );
    assert_eq!(first["matches"]["items"][0]["path"], "src/a.py");
    assert_eq!(first["matches"]["has_more"], true);
    let second = m.ok(
        "ast_grep_search",
        json!({"pattern":"local_value = $EXPR","language":"python","path":"src","limit":1,"offset":first["matches"]["next_offset"],"generation":first["matches"]["generation"]}),
    );
    assert_eq!(second["matches"]["items"][0]["path"], "src/b.py");
    let absent = m.ok(
        "ast_grep_search",
        json!({"pattern":"absent_value = $EXPR","language":"python"}),
    );
    assert_eq!(absent["matches"]["total"], 0);
}

#[test]
fn ast_grep_prefilter_preserves_matches_for_body_only_literals() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("src/a_candidate.py", "def marker():\n    return 1\n");
    w.write("src/z_body.py", "def other():\n    return \"marker\"\n");
    w.build();
    let mut m = Mcp::new(&w);
    let result = m.ok(
        "ast_grep_search",
        json!({"pattern":"return \"marker\"","language":"python"}),
    );
    let items = result["matches"]["items"].as_array().unwrap();
    assert!(
        items.iter().any(|item| item["path"] == "src/z_body.py"),
        "{result}"
    );
}

#[test]
fn removal_proof_normalizes_file_selector_aliases_before_scoping() {
    use serde_json::json;
    let w = Workspace::new();
    w.write(
        "target.py",
        "def target(): return missing_target()\ndef spare(): return 1\n",
    );
    w.write(
        "caller_a.py",
        "from target import target\ndef caller_a(): return target()\n",
    );
    w.write(
        "caller_b.py",
        "from target import target\ndef caller_b(): return target()\n",
    );
    w.build();
    let mut m = Mcp::new(&w);
    let full = m.ok(
        "code_map_prove_removal",
        json!({"selector":"target.py","limit":100}),
    );
    let expected_selected = full["selected_ids"]["items"].as_array().unwrap().clone();
    let expected_dependencies = full["incoming_dependencies"]["items"]
        .as_array()
        .unwrap()
        .clone();
    let selected_total = full["selected_ids"]["total"].as_u64().unwrap();
    let dependency_total = full["incoming_dependencies"]["total"].as_u64().unwrap();
    assert!(selected_total > 1);
    assert!(dependency_total > 1);
    assert!(full["unresolved_references"].as_u64().unwrap() > 0);

    for selector in [
        "target.py",
        "./target.py",
        "file:target.py",
        "file://target.py",
    ] {
        let first = m.ok(
            "code_map_prove_removal",
            json!({"selector":selector,"limit":1,"offset":0}),
        );
        let generation = first["selected_ids"]["generation"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut selected_items = Vec::new();
        let mut dependency_items = Vec::new();
        let last_offset = selected_total.max(dependency_total) as usize;
        for offset in 0..=last_offset {
            let result = if offset == 0 {
                first.clone()
            } else {
                m.ok(
                    "code_map_prove_removal",
                    json!({"selector":selector,"limit":1,"offset":offset,"generation":generation}),
                )
            };
            if (offset as u64) < selected_total {
                assert_eq!(
                    result["selected_ids"]["generation"], generation,
                    "{selector}: {result}"
                );
            }
            assert_eq!(result["selected_ids"]["total"], selected_total);
            assert_eq!(result["incoming_dependencies"]["total"], dependency_total);
            assert_eq!(result["safe_to_remove"], false, "{selector}: {result}");
            assert_eq!(
                result["unresolved_references"],
                full["unresolved_references"]
            );
            assert_eq!(result["parse_errors"], full["parse_errors"]);
            assert_eq!(
                result["assessment"]["blocking_reasons"][0]["count"],
                result["incoming_dependencies"]["total"],
                "{selector}: {result}"
            );
            selected_items.extend(
                result["selected_ids"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            dependency_items.extend(
                result["incoming_dependencies"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        assert_eq!(selected_items, expected_selected, "{selector}");
        assert_eq!(dependency_items, expected_dependencies, "{selector}");
    }
}

#[test]
fn mcp_analyze_router_checkpoint_and_guide_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write("other.py", "def idle(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);

    let attachment = w.db().parent().unwrap().join("sql-attachment.sqlite");
    let mut targets = vec![
        "; DROP TABLE nodes; --".to_owned(),
        "INSERT INTO metadata(key,value) VALUES ('sql-audit-sentinel','changed')".to_owned(),
        "UPDATE nodes SET kind='hacked'".to_owned(),
        "UPDATE metadata SET value='mutated' WHERE key='workspace_root'".to_owned(),
        "DELETE FROM nodes".to_owned(),
        "DELETE FROM files".to_owned(),
        "DROP TABLE nodes".to_owned(),
        "PRAGMA user_version=32767".to_owned(),
        "PRAGMA journal_mode=WAL".to_owned(),
    ];
    targets.push(format!("ATTACH DATABASE '{}' AS pwn", attachment.display()));
    let before = persisted_state(&w);
    for target in &targets {
        let (_, response) = m.call("code_map_analyze", json!({"target":target}));
        assert_eq!(
            response["result"]["isError"], true,
            "unsafe target accepted: {target}: {response}"
        );
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains("write or administrative SQL is not allowed")
                || message.contains("exactly one SELECT or WITH statement is allowed"),
            "{target}: {message}"
        );
        assert_eq!(
            persisted_state(&w),
            before,
            "SQL changed database state: {target}"
        );
        assert!(
            !attachment.exists(),
            "SQL created attached database: {target}"
        );
    }
    let multi_statement = "SELECT count(*) FROM nodes; DROP TABLE nodes";
    m.err(
        "code_map_analyze",
        json!({"target":multi_statement}),
        "exactly one SELECT or WITH statement is allowed",
    );
    assert_eq!(
        persisted_state(&w),
        before,
        "multi-statement SQL changed database state"
    );
    let count = m.ok(
        "code_map_analyze",
        json!({"target":"SELECT count(*) AS count FROM nodes","limit":1}),
    );
    assert!(count["rows"]["items"][0]["count"].as_u64().unwrap() >= 2);
    let paged = m.ok(
        "code_map_analyze",
        json!({"target":"SELECT id FROM nodes ORDER BY id","limit":1}),
    );
    assert_eq!(paged["rows"]["items"].as_array().unwrap().len(), 1);
    assert!(paged["rows"]["total"].as_u64().unwrap() > 1);
    for target in ["", "cycle.py", "other.py"] {
        let off = m.ok(
            "code_map_analyze",
            json!({"target":target,"include_cycles":false}),
        );
        assert_eq!(off["cycles"]["omitted"], true);
        let on = m.ok(
            "code_map_analyze",
            json!({"target":target,"include_cycles":true}),
        );
        if target == "other.py" {
            assert_eq!(on["cycles"]["total"], 0);
        } else {
            assert!(on["cycles"]["total"].as_u64().unwrap() >= 1);
        }
    }

    for (operation, selector) in [
        ("overview", ""),
        ("inspect", "cycle.py:a"),
        ("explain", "cycle.py:a"),
        ("impact", "cycle.py:a"),
        ("slice", "cycle.py:a"),
        ("unwired", ""),
        ("sql", "SELECT count(*) AS total FROM nodes"),
        ("doctor", ""),
        ("search", "a"),
    ] {
        m.ok(
            "code_map_query",
            json!({"operation":operation,"selector":selector,"depth":1,"limit":5}),
        );
    }
    m.err(
        "code_map_query",
        json!({"operation":"cypher","selector":"MATCH (n) RETURN n"}),
        "supported: overview,inspect,explain,impact,slice,unwired,sql",
    );
    m.err(
        "code_map_query",
        json!({"operation":"sql","selector":"SELECT 1; SELECT 2"}),
        "exactly one SELECT or WITH statement is allowed",
    );
    m.err(
        "code_map_query",
        json!({"operation":"sql"}),
        "sql operation requires a SELECT or WITH selector",
    );
    m.err(
        "code_map_query",
        json!({"operation":"delete_all"}),
        "supported: overview,inspect,explain,impact,slice,unwired,sql",
    );

    assert!(m
        .ok("session_checkpoint", json!({"action":"list"}))
        .is_object());
    m.err(
        "session_checkpoint",
        json!({"action":"save","content":{"note":"x"}}),
        "name",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"note"}),
        "content required",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"missing"}),
        "not found",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"delete","name":"missing"}),
        "not found",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"../../etc/passwd","content":{"note":"x"}}),
        "path traversal",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"..\n/../etc/passwd","content":{"note":"x"}}),
        "path traversal",
    );
    m.ok(
        "session_checkpoint",
        json!({"action":"save","name":"note","content":{"state":1}}),
    );
    assert_eq!(
        m.ok("session_checkpoint", json!({"action":"get","name":"note"}))["state"],
        1
    );
    assert_eq!(
        m.ok("session_checkpoint", json!({"action":"list"})),
        json!({"note":{"bytes":11}})
    );
    m.ok(
        "session_checkpoint",
        json!({"action":"delete","name":"note"}),
    );
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"note"}),
        "not found",
    );
    let malformed = run_cli(
        &w,
        &[
            "checkpoint",
            "save",
            "--name",
            "malformed",
            "--content",
            "{invalid",
        ],
    );
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("key must be a string"));
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"malformed"}),
        "not found",
    );
    w.write(".forge/checkpoints.json", "{invalid JSON");
    m.err(
        "session_checkpoint",
        json!({"action":"list"}),
        "key must be a string",
    );

    let guide = m.ok("forge_guide", json!({"topic":"query"}));
    assert!(guide["start"].is_string());
    let acdd = m.ok("forge_guide", json!({"topic":"acdd"}));
    assert!(acdd["gates"].is_array());
    assert!(acdd["proof_policies"].is_object());
    m.err(
        "forge_guide",
        json!({"topic":"no-such-topic"}),
        "unknown guide topic",
    );
}
