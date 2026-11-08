use super::*;

#[test]
fn mcp_graph_depth_cycles_and_direction_boundaries() {
    let w = Workspace::with_files(&[]);
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write(
        "tests/test_cycle.py",
        "from cycle import a\ndef test_a(): a()\n",
    );

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
    w.build();
    let mut m = McpTestHarness::new(&w).expect("MCP initialize");
    for depth in [0, 1, 16] {
        let result = m
            .payload(
                "code_map_impact",
                json!({"selector":"cycle.py:a","depth":depth}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(result["depth"], depth);
        assert!(result["nodes"]["total"].as_u64().unwrap() >= 1);
    }
    let rejection = m
        .text(
            "code_map_impact",
            json!({"selector":"cycle.py:a","depth":17}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "depth must be <=16";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let inbound = m
        .payload(
            "code_map_tests",
            json!({"selector":"cycle.py:a","direction":"inbound"}),
        )
        .expect("MCP tool should succeed");
    assert!(crate::json_array(&inbound["nodes"]["items"])
        .iter()
        .any(|node| node["name"] == "test_a"));
    let test = crate::json_array(&inbound["nodes"]["items"])
        .iter()
        .find(|node| node["name"] == "test_a")
        .unwrap();
    assert_eq!(test["connection"]["relation"], "direct");
    assert_eq!(test["connection"]["edge_kind"], "calls");
    let outbound = m
        .payload(
            "code_map_tests",
            json!({"selector":"tests/test_cycle.py:test_a","direction":"outbound"}),
        )
        .expect("MCP tool should succeed");
    assert!(crate::json_array(&outbound["nodes"]["items"])
        .iter()
        .any(|node| node["name"] == "a"));
    let rejection = m
        .text(
            "code_map_tests",
            json!({"selector":"cycle.py:a","direction":"sideways"}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "direction must be inbound or outbound";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let explained = m
        .explain("cycle.py:a", json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(explained["node"]["kind"], "function");
    let removal = m
        .payload("code_map_prove_removal", json!({"selector":"cycle.py:a"}))
        .expect("MCP tool should succeed");
    assert_eq!(removal["safe_to_remove"], false);

    // Git ranges enter the same real impact service through both interfaces.
    // Keep odd UTF-8/control-character paths intact rather than parsing Git's
    // human-readable quoted headers.
    w.seed_git().expect("seed Git fixture");
    for mode in ["staged", "unstaged", "head"] {
        let empty = m
            .payload("code_map_impact", json!({"changes":mode}))
            .expect("MCP tool should succeed");
        assert_eq!(empty["changes"]["mode"], mode);
        assert_eq!(empty["complete"], true);
        for field in ["changed_symbols", "nodes", "tests", "unmapped_changes"] {
            assert_eq!(empty[field]["total"], 0, "{mode}/{field}: {empty}");
        }
    }
    w.write("cycle.py", "def a(): b(); return 1\ndef b(): a()\n");
    w.write(odd_path, "def odd(): return 1\n");
    w.stage_git_files(&["cycle.py", odd_path]).expect("stage changed files");
    w.write("other.py", "def idle(): return 2\n");
    w.write("untracked.py", "def untracked(): return 5\n");
    w.delta(&["cycle.py", odd_path, "other.py", "untracked.py"]);
    for (mode, expected) in [
        ("staged", vec!["cycle.py", odd_path]),
        ("unstaged", vec!["other.py"]),
        ("head", vec!["cycle.py", odd_path, "other.py"]),
    ] {
        let result = m
            .payload(
                "code_map_impact",
                json!({"changes":mode,"depth":16,"limit":100}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(result["complete"], true, "{mode}: {result}");
        assert_eq!(result["unmapped_changes"]["total"], 0);
        let symbols = crate::json_array(&result["changed_symbols"]["items"]);
        let actual = symbols
            .iter()
            .map(|node| node["path"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected.into_iter().collect(), "{mode}: {result}");
        for node in symbols {
            let inspected = m
                .payload("code_map_inspect", json!({"selector":node["id"]}))
                .expect("MCP tool should succeed");
            assert_eq!(inspected["node"]["path"], node["path"]);
            assert_eq!(inspected["node"]["line"], node["line"]);
        }
        if mode != "unstaged" {
            assert!(crate::json_array(&result["nodes"]["items"])
                .iter()
                .any(|node| node["name"] == "b"));
            let test = crate::json_array(&result["tests"]["items"])
                .iter()
                .find(|node| node["name"] == "test_a")
                .unwrap();
            assert_eq!(test["test_role"], "test_case");
            assert!(crate::json_array(&test["connection"]["call_path"]).len() >= 2);
        }
        let cli = w
            .query(&[
                "query",
                "impact",
                "--changes",
                mode,
                "--depth",
                "16",
                "--limit",
                "100",
            ])
            .expect("CLI should succeed");
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
    let cli_first = w
        .query(&[
            "query",
            "impact",
            "--changes",
            "staged",
            "--depth",
            "16",
            "--limit",
            "1",
        ])
        .expect("CLI should succeed");
    assert_eq!(cli_first["changed_symbols"]["has_more"], true);
    let cli_offset = cli_first["changed_symbols"]["next_offset"]
        .as_u64()
        .unwrap()
        .to_string();
    let cli_generation = cli_first["changed_symbols"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    let cli_next = w
        .query(&[
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
        ])
        .expect("CLI should succeed");
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
        let (_, rejected) = m.call("code_map_impact", arguments).expect("MCP transport");
        assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    }
    let before_invalid = persisted_state(&w).expect("capture persisted SQLite state");
    assert!(!run_cli(&w, &["query", "impact", "--changes", "invalid"])
        .status
        .success());
    assert_eq!(
        persisted_state(&w).expect("capture persisted SQLite state"),
        before_invalid
    );

    // Only the Git index changes here: source bytes and graph generation stay
    // fixed, so ordinary index-generation validation cannot catch this drift.
    let first = m
        .payload("code_map_impact", json!({"changes":"staged","limit":1}))
        .expect("MCP tool should succeed");
    assert_eq!(first["changed_symbols"]["has_more"], true);
    let token = first["changed_symbols"]["generation"].clone();
    let next_offset = first["changed_symbols"]["next_offset"].clone();
    let next = m
        .payload(
            "code_map_impact",
            json!({"changes":"staged","limit":1,"offset":next_offset,"generation":token}),
        )
        .expect("MCP tool should succeed");
    assert_ne!(
        first["changed_symbols"]["items"][0]["id"],
        next["changed_symbols"]["items"][0]["id"]
    );
    let graph = m
        .payload("code_map_overview", json!({}))
        .expect("MCP tool should succeed")["generation"]
        .clone();
    w.stage_git_files(&["other.py"]).expect("stage other.py");
    assert_eq!(
        m.payload("code_map_overview", json!({}))
            .expect("MCP tool should succeed")["generation"],
        graph
    );
    let rejection = m
        .text(
            "code_map_impact",
            json!({"changes":"staged","limit":1,"offset":next_offset,"generation":token}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "changed";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );

    // Staged hunks describe the index postimage, not these newly shifted
    // working-tree ranges. Indexing the latter cannot make mapping lawful.
    w.write(
        "cycle.py",
        "# shifted\ndef a(): b(); return 1\ndef b(): a()\n",
    );
    w.build();
    let mismatch = m
        .payload("code_map_impact", json!({"changes":"staged","limit":100}))
        .expect("MCP tool should succeed");
    assert_eq!(mismatch["complete"], false);
    assert!(crate::json_array(&mismatch["unmapped_changes"]["items"])
        .iter()
        .any(|item| item["path"] == "cycle.py" && item["reason"] == "snapshot_mismatch"));
    assert!(!crate::json_array(&mismatch["changed_symbols"]["items"])
        .iter()
        .any(|item| item["path"] == "cycle.py"));

    w.stage_git_files(&["cycle.py"]).expect("stage cycle.py");
    w.commit_git("Accept mapped changes", None).expect("commit mapped changes");
    fs::rename(w.path("renamed.py"), w.path("moved.py")).unwrap();
    w.write("deleted.py", "def neighbor(): return 1\n");
    w.write_bytes("binary.dat", &[0, 3, 4]);
    w.stage_git_files(&["renamed.py", "moved.py", "deleted.py", "binary.dat"])
        .expect("stage removed, renamed, and binary paths");
    w.build();
    let unmapped = m
        .payload("code_map_impact", json!({"changes":"staged","limit":100}))
        .expect("MCP tool should succeed");
    assert_eq!(unmapped["complete"], false);
    for (path, reason) in [
        ("deleted.py", "deleted_range"),
        ("binary.dat", "binary"),
        ("moved.py", "rename_only"),
    ] {
        assert!(
            crate::json_array(&unmapped["unmapped_changes"]["items"])
                .iter()
                .any(|item| item["path"] == path && item["reason"] == reason),
            "{path}/{reason}: {unmapped}"
        );
    }
    assert!(!crate::json_array(&unmapped["changed_symbols"]["items"])
        .iter()
        .any(|item| item["path"] == "deleted.py" || item["path"] == "moved.py"));
    w.write("moved.py", "def renamed():\n    first = 3\n    second = 1\n    third = 2\n    return first + second + third\n");
    w.stage_git_files(&["moved.py"]).expect("stage moved.py");
    w.build();
    let renamed = m
        .payload("code_map_impact", json!({"changes":"staged","limit":100}))
        .expect("MCP tool should succeed");
    assert!(crate::json_array(&renamed["changed_symbols"]["items"])
        .iter()
        .any(|item| item["path"] == "moved.py" && item["name"] == "renamed"));
    assert!(crate::json_array(&renamed["changes"]["files"])
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
            source.push_str(&format!("def repeat_{index}(): seed(); repeat_{index}()\n"));
        }
        source.push_str("def tail_0(): seed()\n");
        for index in 1..=15 {
            source.push_str(&format!("def tail_{index}(): tail_{}()\n", index - 1));
        }
        source
    };
    w.delta(&["decor.py", "prefix.ts", "removed.ts"]);
    w.run_git(&["add", "-A"]).expect("stage review boundary fixture");
    w.commit_git("Review boundary fixture baseline", None)
        .expect("commit review boundary fixture");

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
    w.stage_git_files(&["prefix.ts"]).expect("stage prefix.ts");
    w.build();
    let (_, response) = m
        .call(
            "code_map_impact",
            json!({"changes":"staged","path":"prefix.ts","format":"json"}),
        )
        .expect("MCP transport");
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
    let (_, response) = m
        .call(
            "code_map_impact",
            json!({"changes":"unstaged","path":"removed.ts","format":"json"}),
        )
        .expect("MCP transport");
    if response["result"]["isError"] == true {
        regression_failures.push(format!("-- source deletion was rejected: {response}"));
    } else if let Some(result) = parse_payload(&response) {
        let deletion_retained =
            result["unmapped_changes"]["items"]
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
    w.stage_git_files(&["decor.py"]).expect("stage decor.py");
    w.build();
    let (_, response) = m
        .call(
            "code_map_impact",
            json!({"changes":"staged","path":"decor.py","depth":2,"limit":100,"format":"json"}),
        )
        .expect("MCP transport");
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
        regression_failures.push(format!(
            "reverse predecessor response was not JSON: {response}"
        ));
    }

    let control_w = Workspace::with_files(&[
        (".gitignore", ".forge/\n"),
        ("graph.py", &graph_source(630)),
    ]);
    control_w.seed_git().expect("seed control Git fixture");
    let control = graph_source(630).replace("def seed(): return 1", "def seed(): return 2");
    control_w.write("graph.py", &control);
    control_w.stage_git_files(&["graph.py"]).expect("stage graph mutation");
    control_w.build();
    let mut control_m = McpTestHarness::new(&control_w).expect("MCP initialize");
    let (_, changed_control_response) = control_m
        .call(
            "code_map_impact",
            json!({"changes":"staged","depth":16,"limit":100,"format":"json"}),
        )
        .expect("MCP transport");
    let (_, selected_control_response) = control_m
        .call(
            "code_map_impact",
            json!({"selector":"graph.py:seed","depth":16,"limit":100,"format":"json"}),
        )
        .expect("MCP transport");
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

    w.write("graph.py", &graph_source(670));
    w.stage_git_files(&["graph.py"]).expect("stage graph.py");
    w.commit_git("Deep graph baseline", Some(&["graph.py"]))
        .expect("commit deep graph baseline");
    let changed_graph = graph_source(670).replace("def seed(): return 1", "def seed(): return 2");
    w.write("graph.py", &changed_graph);
    w.stage_git_files(&["graph.py"]).expect("stage graph change");
    w.build();
    let impact_args =
        json!({"changes":"staged","path":"graph.py","depth":16,"limit":100,"format":"json"});
    let (_, changed_graph_response) = m
        .call("code_map_impact", impact_args.clone())
        .expect("MCP transport");
    let (_, selected_graph_response) = m
        .call(
            "code_map_impact",
            json!({"selector":"graph.py:seed","depth":16,"limit":100,"format":"json"}),
        )
        .expect("MCP transport");
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
                json!({"changes":"staged","path":"graph.py","depth":16,"limit":100,"offset":600,"generation":generation,"format":"json"}),
            ).expect("MCP transport");
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
    let rejection = m
        .text("code_map_impact", json!({"changes":"head"}))
        .expect_err("tool should reject input");
    let expected: &str = "Git";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    control_m.finish().expect("server must finish successfully");
    m.finish().expect("server must finish successfully");
}

#[test]
fn analyze_cycle_controls_respect_whole_workspace_and_exact_file_scope() {
    let fixture = AnalysisFixture::indexed();
    let mut m = fixture
        .start_mcp()
        .expect("indexed MCP fixture should initialize");
    for target in ["", "cycle.py", "other.py"] {
        let off = m
            .payload(
                "code_map_analyze",
                json!({"target":target,"include_cycles":false}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(off["cycles"]["omitted"], true);
        let on = m
            .payload(
                "code_map_analyze",
                json!({"target":target,"include_cycles":true}),
            )
            .expect("MCP tool should succeed");
        if target == "other.py" {
            assert_eq!(on["cycles"]["total"], 0);
        } else {
            assert!(on["cycles"]["total"].as_u64().unwrap() >= 1);
        }
    }
    m.finish().expect("server must finish successfully");
}
