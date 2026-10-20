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

    // Git ranges enter the same real impact service through both interfaces.
    // Keep odd UTF-8/control-character paths intact rather than parsing Git's
    // human-readable quoted headers.
    use contextunity_forge_mcp::core::git::git_command;
    let git = |args: &[&str]| {
        let output = git_command(w.root(), args).unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    w.write(".gitignore", ".forge/\n");
    w.write("other.py", "def idle(): return 0\n");
    let odd_path = "paths/space α\tline\nname.py";
    w.write(odd_path, "def odd(): return 0\n");
    w.write("renamed.py", "def renamed():\n    first = 0\n    second = 1\n    third = 2\n    return first + second + third\n");
    w.write(
        "deleted.py",
        "def deleted(): return 0\n\ndef neighbor(): return 1\n",
    );
    w.write_bytes("binary.dat", &[0, 1, 2]);
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.email", "contract@example.invalid"],
        vec!["config", "user.name", "Contract fixture"],
        vec![
            "add",
            "--",
            ".gitignore",
            "cycle.py",
            "tests/test_cycle.py",
            "other.py",
            odd_path,
            "renamed.py",
            "deleted.py",
            "binary.dat",
        ],
        vec!["commit", "--quiet", "-m", "Fixture baseline"],
    ] {
        git(&args);
    }
    w.build();
    for mode in ["staged", "unstaged", "head"] {
        let empty = m.ok("code_map_impact", json!({"changes":mode}));
        assert_eq!(empty["changes"]["mode"], mode);
        assert_eq!(empty["complete"], true);
        for field in ["changed_symbols", "nodes", "tests", "unmapped_changes"] {
            assert_eq!(empty[field]["total"], 0, "{mode}/{field}: {empty}");
        }
    }
    w.write("cycle.py", "def a(): b(); return 1\ndef b(): a()\n");
    w.write(odd_path, "def odd(): return 1\n");
    git(&["add", "--", "cycle.py", odd_path]);
    w.write("other.py", "def idle(): return 2\n");
    w.write("untracked.py", "def untracked(): return 5\n");
    w.build();
    for (mode, expected) in [
        ("staged", vec!["cycle.py", odd_path]),
        ("unstaged", vec!["other.py"]),
        ("head", vec!["cycle.py", odd_path, "other.py"]),
    ] {
        let result = m.ok(
            "code_map_impact",
            json!({"changes":mode,"depth":16,"limit":100}),
        );
        assert_eq!(result["complete"], true, "{mode}: {result}");
        assert_eq!(result["unmapped_changes"]["total"], 0);
        let symbols = result["changed_symbols"]["items"].as_array().unwrap();
        let actual = symbols
            .iter()
            .map(|node| node["path"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected.into_iter().collect(), "{mode}: {result}");
        for node in symbols {
            let inspected = m.ok("code_map_inspect", json!({"selector":node["id"]}));
            assert_eq!(inspected["node"]["path"], node["path"]);
            assert_eq!(inspected["node"]["line"], node["line"]);
        }
        if mode != "unstaged" {
            assert!(result["nodes"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["name"] == "b"));
            let test = result["tests"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["name"] == "test_a")
                .unwrap();
            assert_eq!(test["test_role"], "test_case");
            assert!(test["connection"]["call_path"].as_array().unwrap().len() >= 2);
        }
        let cli = w.query(&[
            "query",
            "impact",
            "--changes",
            mode,
            "--depth",
            "16",
            "--limit",
            "100",
        ]);
        for field in ["changed_symbols", "nodes", "tests"] {
            assert_eq!(
                cli[field]["total"], result[field]["total"],
                "{mode}/{field}"
            );
            let cli_ids = cli[field]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["id"].as_str().unwrap())
                .collect::<Vec<_>>();
            let mcp_ids = result[field]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["id"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(cli_ids, mcp_ids, "{mode}/{field}: CLI/MCP parity");
        }
        assert_eq!(cli["complete"], result["complete"]);
        assert_eq!(cli["changes"], result["changes"]);
    }
    let cli_first = run_cli(
        &w,
        &[
            "query",
            "impact",
            "--changes",
            "staged",
            "--depth",
            "16",
            "--limit",
            "1",
        ],
    );
    assert!(
        cli_first.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_first.stderr)
    );
    let cli_first: serde_json::Value = serde_json::from_slice(&cli_first.stdout).unwrap();
    assert_eq!(cli_first["changed_symbols"]["has_more"], true);
    let cli_offset = cli_first["changed_symbols"]["next_offset"]
        .as_u64()
        .unwrap()
        .to_string();
    let cli_generation = cli_first["changed_symbols"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    let cli_next = run_cli(
        &w,
        &[
            "query",
            "impact",
            "--changes",
            "staged",
            "--depth",
            "16",
            "--limit",
            "1",
            "--offset",
            &cli_offset,
            "--generation",
            &cli_generation,
        ],
    );
    assert!(
        cli_next.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_next.stderr)
    );
    let cli_next: serde_json::Value = serde_json::from_slice(&cli_next.stdout).unwrap();
    assert_ne!(
        cli_first["changed_symbols"]["items"][0]["id"],
        cli_next["changed_symbols"]["items"][0]["id"]
    );
    for arguments in [
        json!({}),
        json!({"selector":"cycle.py:a","changes":"head"}),
        json!({"changes":"invalid"}),
        json!({"changes":"head","path":"../outside"}),
    ] {
        let (_, rejected) = m.call("code_map_impact", arguments);
        assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    }
    let before_invalid = persisted_state(&w);
    assert!(!run_cli(&w, &["query", "impact", "--changes", "invalid"])
        .status
        .success());
    assert_eq!(persisted_state(&w), before_invalid);

    // Only the Git index changes here: source bytes and graph generation stay
    // fixed, so ordinary index-generation validation cannot catch this drift.
    let first = m.ok("code_map_impact", json!({"changes":"staged","limit":1}));
    assert_eq!(first["changed_symbols"]["has_more"], true);
    let token = first["changed_symbols"]["generation"].clone();
    let next_offset = first["changed_symbols"]["next_offset"].clone();
    let next = m.ok(
        "code_map_impact",
        json!({"changes":"staged","limit":1,"offset":next_offset,"generation":token}),
    );
    assert_ne!(
        first["changed_symbols"]["items"][0]["id"],
        next["changed_symbols"]["items"][0]["id"]
    );
    let graph = m.ok("code_map_overview", json!({}))["generation"].clone();
    git(&["add", "--", "other.py"]);
    assert_eq!(m.ok("code_map_overview", json!({}))["generation"], graph);
    m.err(
        "code_map_impact",
        json!({"changes":"staged","limit":1,"offset":next_offset,"generation":token}),
        "changed",
    );

    // Staged hunks describe the index postimage, not these newly shifted
    // working-tree ranges. Indexing the latter cannot make mapping lawful.
    w.write(
        "cycle.py",
        "# shifted\ndef a(): b(); return 1\ndef b(): a()\n",
    );
    w.build();
    let mismatch = m.ok("code_map_impact", json!({"changes":"staged","limit":100}));
    assert_eq!(mismatch["complete"], false);
    assert!(mismatch["unmapped_changes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == "cycle.py" && item["reason"] == "snapshot_mismatch"));
    assert!(!mismatch["changed_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == "cycle.py"));

    git(&["add", "--", "cycle.py"]);
    git(&["commit", "--quiet", "-m", "Accept mapped changes"]);
    fs::rename(w.path("renamed.py"), w.path("moved.py")).unwrap();
    w.write("deleted.py", "def neighbor(): return 1\n");
    w.write_bytes("binary.dat", &[0, 3, 4]);
    git(&[
        "add",
        "--",
        "renamed.py",
        "moved.py",
        "deleted.py",
        "binary.dat",
    ]);
    w.build();
    let unmapped = m.ok("code_map_impact", json!({"changes":"staged","limit":100}));
    assert_eq!(unmapped["complete"], false);
    for (path, reason) in [
        ("deleted.py", "deleted_range"),
        ("binary.dat", "binary"),
        ("moved.py", "rename_only"),
    ] {
        assert!(
            unmapped["unmapped_changes"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["path"] == path && item["reason"] == reason),
            "{path}/{reason}: {unmapped}"
        );
    }
    assert!(!unmapped["changed_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == "deleted.py" || item["path"] == "moved.py"));
    w.write("moved.py", "def renamed():\n    first = 3\n    second = 1\n    third = 2\n    return first + second + third\n");
    git(&["add", "--", "moved.py"]);
    w.build();
    let renamed = m.ok("code_map_impact", json!({"changes":"staged","limit":100}));
    assert!(renamed["changed_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == "moved.py" && item["name"] == "renamed"));
    assert!(renamed["changes"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == "moved.py" && item["old_path"] == "renamed.py"));

    w.write("decor.py", "def helper(): return 1\ndef deco(fn):\n    helper()\n    return fn\n@deco\ndef target(): pass\n");
    w.write(
        "prefix.ts",
        "export function counter() {\nlet value = 0;\nreturn value;\n}\n",
    );
    w.write(
        "removed.ts",
        "export function decrement() {\nlet value = 2;\n--value;\nreturn value;\n}\n",
    );
    let graph_source = |repeat_count: usize| {
        let mut source = String::from("def seed(): return 1\n");
        for index in 0..repeat_count {
            source.push_str(&format!(
                "def repeat_{index}(): seed(); repeat_{index}()\n"
            ));
        }
        source.push_str("def tail_0(): seed()\n");
        for index in 1..=15 {
            source.push_str(&format!("def tail_{index}(): tail_{}()\n", index - 1));
        }
        source
    };
    w.write("graph.py", &graph_source(670));
    w.build();
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "Review boundary fixture baseline"]);

    let mut regression_failures = Vec::new();
    let parse_payload = |response: &serde_json::Value| -> Option<serde_json::Value> {
        response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
    };

    w.write(
        "prefix.ts",
        "export function counter() {\nlet value = 0;\n++value;\nreturn value;\n}\n",
    );
    git(&["add", "--", "prefix.ts"]);
    w.build();
    let (_, response) = m.call(
        "code_map_impact",
        json!({"changes":"staged","path":"prefix.ts"}),
    );
    if response["result"]["isError"] == true {
        regression_failures.push(format!("++ source line was rejected: {response}"));
    } else if let Some(result) = parse_payload(&response) {
        let counter_mapped = result["changed_symbols"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["name"] == "counter"));
        if result["complete"] != true || !counter_mapped {
            regression_failures.push(format!(
                "++ source addition was not mapped completely: symbols={}, unmapped={}, complete={}",
                result["changed_symbols"]["total"],
                result["unmapped_changes"]["items"],
                result["complete"]
            ));
        }
    } else {
        regression_failures.push(format!("++ source response was not JSON: {response}"));
    }

    w.write(
        "removed.ts",
        "export function decrement() {\nlet value = 2;\nreturn value;\n}\n",
    );
    w.build();
    let (_, response) = m.call(
        "code_map_impact",
        json!({"changes":"unstaged","path":"removed.ts"}),
    );
    if response["result"]["isError"] == true {
        regression_failures.push(format!("-- source deletion was rejected: {response}"));
    } else if let Some(result) = parse_payload(&response) {
        let deletion_retained = result["unmapped_changes"]["items"]
            .as_array()
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item["path"] == "removed.ts" && item["reason"] == "deleted_range"
                })
            });
        if result["complete"] != false || !deletion_retained {
            regression_failures.push(format!(
                "-- source deletion was not retained as incomplete evidence: unmapped={}, complete={}",
                result["unmapped_changes"]["items"],
                result["complete"]
            ));
        }
    } else {
        regression_failures.push(format!("-- source response was not JSON: {response}"));
    }

    w.write(
        "decor.py",
        "def helper(): return 2\ndef deco(fn):\n    helper()\n    return fn\n@deco\ndef target(): pass\n",
    );
    git(&["add", "--", "decor.py"]);
    w.build();
    let (_, response) = m.call(
        "code_map_impact",
        json!({"changes":"staged","path":"decor.py","depth":2,"limit":100}),
    );
    if response["result"]["isError"] == true {
        regression_failures.push(format!("reverse predecessor response failed: {response}"));
    } else if let Some(result) = parse_payload(&response) {
        let target = result["nodes"]["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "target"));
        if target.is_none_or(|node| {
            node["distance"] != 2
                || node["via"]["id"] != "py:decor.py:2:deco"
                || node["via"]["kind"] != "decorates"
        }) {
            regression_failures.push(format!(
                "reverse predecessor did not retain public via identity: target={target:?}, result={}",
                result["nodes"]["items"]
            ));
        }
    } else {
        regression_failures.push(format!("reverse predecessor response was not JSON: {response}"));
    }

    let control_w = Workspace::new();
    control_w.write(".gitignore", ".forge/\n");
    control_w.write("graph.py", &graph_source(630));
    let control_git = |args: &[&str]| {
        let output = git_command(control_w.root(), args).unwrap();
        assert!(
            output.status.success(),
            "control {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.email", "contract@example.invalid"],
        vec!["config", "user.name", "Contract fixture"],
        vec!["add", "--", ".gitignore", "graph.py"],
        vec!["commit", "--quiet", "-m", "Small graph control baseline"],
    ] {
        control_git(&args);
    }
    control_w.build();
    let control = graph_source(630).replace("def seed(): return 1", "def seed(): return 2");
    control_w.write("graph.py", &control);
    control_git(&["add", "--", "graph.py"]);
    control_w.build();
    let mut control_m = Mcp::new(&control_w);
    let (_, changed_control_response) = control_m.call(
        "code_map_impact",
        json!({"changes":"staged","depth":16,"limit":100}),
    );
    let (_, selected_control_response) = control_m.call(
        "code_map_impact",
        json!({"selector":"graph.py:seed","depth":16,"limit":100}),
    );
    match (
        changed_control_response["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        selected_control_response["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        parse_payload(&changed_control_response),
        parse_payload(&selected_control_response),
    ) {
        (false, false, Some(changed), Some(selected)) => {
            if changed["complete"] != true
                || changed["nodes"]["total"] != 649
                || changed["nodes"]["total"] != selected["nodes"]["total"]
            {
                regression_failures.push(format!(
                    "smaller positive graph control changed/selector totals differ: changed={} selected={}",
                    changed["nodes"]["total"],
                    selected["nodes"]["total"]
                ));
            }
        }
        _ => regression_failures.push(format!(
            "smaller positive graph control failed: changed={changed_control_response} selected={selected_control_response}"
        )),
    }

    let changed_graph = graph_source(670).replace("def seed(): return 1", "def seed(): return 2");
    w.write("graph.py", &changed_graph);
    git(&["add", "--", "graph.py"]);
    w.build();
    let impact_args = json!({"changes":"staged","path":"graph.py","depth":16,"limit":100});
    let (_, changed_graph_response) = m.call("code_map_impact", impact_args.clone());
    let (_, selected_graph_response) = m.call(
        "code_map_impact",
        json!({"selector":"graph.py:seed","depth":16,"limit":100}),
    );
    match (
        changed_graph_response["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        selected_graph_response["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        parse_payload(&changed_graph_response),
        parse_payload(&selected_graph_response),
    ) {
        (false, false, Some(changed), Some(selected)) => {
            let generation = changed["nodes"]["generation"].clone();
            let (_, tail_response) = m.call(
                "code_map_impact",
                json!({"changes":"staged","path":"graph.py","depth":16,"limit":100,"offset":600,"generation":generation}),
            );
            let tail = parse_payload(&tail_response);
            let tail_15_present = tail.as_ref().is_some_and(|page| {
                page["nodes"]["items"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item["name"] == "tail_15"))
            });
            if changed["complete"] != true
                || changed["nodes"]["total"] != 689
                || changed["nodes"]["total"] != selected["nodes"]["total"]
                || !tail_15_present
            {
                regression_failures.push(format!(
                    "deep cyclic graph was partial: changed_total={} selected_total={} complete={} tail_15_present={} tail_response={tail_response}",
                    changed["nodes"]["total"],
                    selected["nodes"]["total"],
                    changed["complete"],
                    tail_15_present
                ));
            }
        }
        _ => regression_failures.push(format!(
            "deep graph impact failed before proving its bounded result: changed={changed_graph_response} selected={selected_graph_response}"
        )),
    }
    assert!(
        regression_failures.is_empty(),
        "review boundary regressions:\n{}",
        regression_failures.join("\n")
    );
    fs::rename(w.path(".git"), w.path(".saved-git")).unwrap();
    m.err("code_map_impact", json!({"changes":"head"}), "Git");
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

    // Granular diagnostics retain the persisted resolution status/evidence and
    // show actual source, including arguments and CRLF/UTF-8.
    w.write("requirements.txt", "pydantic\nrequests\n");
    let diagnostics = concat!(
        "from pydantic import BaseModel\r\n",
        "from pathlib import Path\r\n",
        "import requests\r\n",
        "class Record(BaseModel):\r\n",
        "    title: str\r\n",
        "def dynamic(callback):\r\n",
        "    callback('é argument')\r\n",
        "def unknown(value):\r\n",
        "    value.run('é argument')\r\n",
        "def consume(record: Record):\r\n",
        "    record.model_dump()\r\n",
        "    Path('é.txt').exists()\r\n",
        "    requests.get('https://example.invalid')\r\n",
        "    missing_target('é argument')\r\n",
        "    Record.model_validate({'title': 'ok'})\r\n",
        "class Known: pass\r\n",
        "def known(value: Known):\r\n",
        "    value.missing('known method')\r\n",
        "def computed(make):\r\n",
        "    make()('computed callback')\r\n",
        "    make().run('computed receiver')\r\n",
    );
    w.write("diagnostics.py", diagnostics);
    w.build();
    let mut diagnostic_failures = Vec::new();
    for detail in ["compact", "full"] {
        let mut offset = 0;
        let mut generation = Value::Null;
        let mut items = Vec::new();
        let mut external = Vec::new();
        loop {
            let result = m.ok("code_map_analyze", json!({"target":"diagnostics.py","detail":detail,"limit":1,"offset":offset,"generation":generation}));
            items.extend(
                result["resolution"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            if result["resolution"]["has_more"] != true {
                break;
            }
            offset = result["resolution"]["next_offset"].as_u64().unwrap();
            generation = result["resolution"]["generation"].clone();
        }
        offset = 0;
        generation = Value::Null;
        loop {
            let result = m.ok("code_map_analyze", json!({"target":"diagnostics.py","detail":detail,"limit":1,"offset":offset,"generation":generation}));
            external.extend(
                result["external_imports"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            if result["external_imports"]["has_more"] != true {
                break;
            }
            offset = result["external_imports"]["next_offset"].as_u64().unwrap();
            generation = result["external_imports"]["generation"].clone();
        }
        for (expression, reason, status, line) in [
            ("callback", "dynamic_callback", "unresolved", 7),
            ("value.run", "unresolved_receiver_type", "unresolved", 9),
            ("record.model_dump", "framework_magic", "external", 11),
            ("requests.get", "external_unindexed", "external", 13),
            ("Record.model_validate", "framework_magic", "external", 15),
        ] {
            let item = items
                .iter()
                .chain(&external)
                .find(|item| item["expression"] == expression && item["line"] == line)
                .unwrap_or_else(|| {
                    panic!("{expression}: resolution={items:?}, external={external:?}")
                });
            assert_eq!(item["status"], status);
            if item["reason"] != reason {
                diagnostic_failures.push(format!(
                    "{detail}/{expression}: expected {reason}, actual={item}"
                ));
            }
            let expected_snippet = diagnostics
                .split_inclusive('\n')
                .nth(line - 1)
                .unwrap()
                .trim_end_matches(['\r', '\n']);
            if item["snippet"]
                .as_str()
                .map(|text| text.trim_end_matches(['\r', '\n']))
                != Some(expected_snippet)
            {
                diagnostic_failures.push(format!(
                    "{detail}/{expression}: expected source {expected_snippet:?}, actual={item}"
                ));
            }
            if status == "unresolved" {
                assert!(item["cause"].is_string());
            }
            if detail == "full" {
                assert!(item["evidence"].is_string());
            }
        }
        let generic = items
            .iter()
            .find(|item| item["expression"] == "missing_target")
            .unwrap();
        assert!(
            generic["reason"].is_null(),
            "generic absence is not framework evidence: {generic}"
        );
        let stdlib = external
            .iter()
            .find(|item| item["line"] == 12)
            .expect("finite pathlib member remains recorded as external");
        assert_ne!(stdlib["reason"], "framework_magic", "{stdlib}");
        for (expression, line) in [("value.missing", 18), ("make()", 20), ("make().run", 21)] {
            let item = items
                .iter()
                .chain(&external)
                .find(|item| item["expression"] == expression && item["line"] == line)
                .unwrap_or_else(|| {
                    panic!("{expression}: resolution={items:?}, external={external:?}")
                });
            assert_eq!(item["status"], "unresolved", "{detail}/{expression}: {item}");
            assert!(item["cause"].is_string(), "{detail}/{expression}: {item}");
            if detail == "full" {
                assert!(item["evidence"].is_string(), "{detail}/{expression}: {item}");
            }
            if !item["reason"].is_null() {
                diagnostic_failures.push(format!(
                    "{detail}/{expression}: unproven category must stay null, actual={item}"
                ));
            }
        }
    }
    let conn = w.open();
    let phantom_edges = reader::rows(&conn,
        "SELECT count(*) AS total FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE p.path='diagnostics.py' AND e.line IN(7,9,11,13,15) AND e.kind IN('calls','possible_calls')", &[], 1).unwrap();
    assert_eq!(
        phantom_edges[0]["total"], 0,
        "diagnostic labels cannot create graph targets"
    );
    drop(conn);

    // Scope identity and counts come from this published snapshot, not from
    // component labels or the current adapter after it has changed.
    let linked = Workspace::new();
    linked.write(
        "src/linked.py",
        "def linked_entry(value): value.missing()\n",
    );
    w.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: linked\n    path: '{}'\n    roots: [src]\n",
            linked.root().display()
        ),
    );
    w.build();
    let full = m.ok("code_map_overview", json!({"limit":100}));
    let scopes = full["coverage_by_workspace"]["items"]
        .as_array()
        .unwrap_or_else(|| {
            panic!(
                "missing persisted workspace breakdown; diagnostics: {}",
                diagnostic_failures.join("\n")
            )
        });
    assert_eq!(full["coverage_by_workspace"]["total"], 2);
    let conn = w.open();
    for (name, root, prefix) in [
        ("primary", w.root(), ""),
        ("linked", linked.root(), "[linked]/"),
    ] {
        let scope = scopes.iter().find(|scope| scope["name"] == name).unwrap();
        assert_eq!(
            scope["root"],
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(scope["path_prefix"], prefix);
        let expected = reader::rows(&conn,
            "SELECT c.status,count(*) total FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE (?1='' AND p.path NOT LIKE '[linked]/%') OR (?1<>'' AND substr(p.path,1,length(?1))=?1) GROUP BY c.status",
            &[&prefix], 10).unwrap();
        for (status, field) in [
            ("resolved", "resolved"),
            ("ambiguous", "ambiguous"),
            ("unresolved", "unresolved"),
            ("external", "external_imports"),
        ] {
            let count = expected
                .iter()
                .find(|row| row["status"] == status)
                .and_then(|row| row["total"].as_u64())
                .unwrap_or(0);
            assert_eq!(scope[field], count, "{name}/{field}: {scope}");
        }
    }
    drop(conn);
    let first = m.ok(
        "code_map_overview",
        json!({"aspects":["coverage_by_workspace"],"limit":1}),
    );
    assert_eq!(first["coverage_by_workspace"]["has_more"], true);
    let next = m.ok("code_map_overview", json!({"aspects":["coverage_by_workspace"],"limit":1,"offset":first["coverage_by_workspace"]["next_offset"],"generation":first["coverage_by_workspace"]["generation"]}));
    let mut paged = vec![
        first["coverage_by_workspace"]["items"][0].clone(),
        next["coverage_by_workspace"]["items"][0].clone(),
    ];
    paged.sort_by_key(|scope| scope["name"].as_str().unwrap().to_owned());
    let mut expected = scopes.clone();
    expected.sort_by_key(|scope| scope["name"].as_str().unwrap().to_owned());
    assert_eq!(paged, expected);
    drop(m);
    let mut restarted = Mcp::new(&w);
    assert_eq!(
        restarted.ok("code_map_overview", json!({"limit":100}))["coverage_by_workspace"],
        full["coverage_by_workspace"]
    );
    drop(restarted);
    w.write("forge-mcp.yaml", "roots: [.]\n");
    let reopened = w.open();
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .unwrap();
    let persisted = reader::overview_with_options(
        &reopened,
        &reader::OverviewOptions {
            aspects: None,
            page: &page,
        },
    )
    .unwrap();
    assert_eq!(
        persisted["coverage_by_workspace"]["items"],
        full["coverage_by_workspace"]["items"]
    );
    assert_eq!(
        persisted["coverage_by_workspace"]["total"],
        full["coverage_by_workspace"]["total"]
    );
    drop(reopened);
    w.build();
    let mut rebuilt = Mcp::new(&w);
    let changed = rebuilt.ok("code_map_overview", json!({"limit":100}));
    assert_eq!(changed["coverage_by_workspace"]["total"], 1);
    assert_eq!(
        changed["coverage_by_workspace"]["items"][0]["name"],
        "primary"
    );
    assert_ne!(changed["generation"], full["generation"]);
    assert!(
        diagnostic_failures.is_empty(),
        "{}",
        diagnostic_failures.join("\n")
    );
}
