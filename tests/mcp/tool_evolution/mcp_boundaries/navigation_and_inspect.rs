use super::*;

#[test]
fn ergonomic_search_parameters_preserve_scope_and_paging() {
    let mut m = McpTestHarness::indexed(&[
        ("src/app.py", "def first(): pass\ndef second(): pass\n"),
        ("other.py", "def outside(): pass\n"),
    ])
    .expect("MCP initialize");
    let canonical = m
        .search("first", json!({}))
        .expect("MCP tool should succeed");
    for args in [
        json!({"query":"first"}),
        json!({"pattern":"first","path":""}),
        json!({"query":"first","path":""}),
    ] {
        let result = m
            .payload("code_map_search", args)
            .expect("MCP tool should succeed");
        assert_eq!(result["nodes"], canonical["nodes"]);
    }
    for args in [
        json!({"pattern":"*","path":"src"}),
        json!({"query":"*","path":"src/app.py"}),
        json!({"pattern":"*","path":"src","exact":true}),
    ] {
        let scope = args["path"].as_str().unwrap().to_owned();
        let result = m
            .payload("code_map_search", args)
            .expect("MCP tool should succeed");
        let hits = crate::json_array(&result["nodes"]["items"]);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| {
            let path = hit["path"].as_str().unwrap();
            path == scope || path.starts_with(&format!("{scope}/"))
        }));
        assert!(hits.iter().any(|hit| hit["name"] == "first"));
        assert!(hits.iter().any(|hit| hit["name"] == "second"));
    }
    let all_functions = m
        .search("*", json!({"kind":"function","path":""}))
        .expect("MCP tool should succeed");
    assert_eq!(all_functions["nodes"]["total"], 3);
    let first = m
        .search("*", json!({"path":"src","kind":"function","limit":1}))
        .expect("MCP tool should succeed");
    assert_eq!(first["nodes"]["total"], 2);
    let next = m.search("*", json!({"path":"src","kind":"function","limit":1,"offset":first["nodes"]["next_offset"],"generation":first["nodes"]["generation"]})).expect("MCP tool should succeed");
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
        json!({"pattern":"_"}),
        json!({"pattern":"%"}),
        json!({"pattern":"\\"}),
        json!({"pattern":"'"}),
        json!({"pattern":"\""}),
    ] {
        let rejection = m
            .text("code_map_search", args)
            .expect_err("tool should reject input");
        let expected: &str = "pattern must contain";
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    for path in ["../src", "/src", "src//app.py"] {
        let rejection = m
            .text("code_map_search", json!({"pattern":"*","path":path}))
            .expect_err("tool should reject input");
        let expected: &str = "workspace-relative";
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    m.finish().expect("server must finish successfully");
}

#[test]
fn ergonomic_source_budgets_preserve_defaults_and_explicit_limits_at_mcp_boundary() {
    let source = format!(
        "{}def preview():\n{}",
        "# context\n".repeat(30),
        "    x=1\n".repeat(510)
    );
    assert!(source.len() < 8 * 1024);
    let mut m = McpTestHarness::indexed(&[("app.py", source.as_str())]).expect("MCP initialize");
    for tool in ["code_map_inspect", "code_map_explain"] {
        let clamped = m.payload(tool, json!({"selector":"app.py:preview","show_source":true,"leading_lines":30,"max_body_lines":501})).expect("MCP tool should succeed");
        let bounded = m.payload(tool, json!({"selector":"app.py:preview","show_source":true,"leading_lines":20,"max_body_lines":500})).expect("MCP tool should succeed");
        assert_eq!(clamped["source"], bounded["source"], "{tool}");
        assert_eq!(
            clamped["source_preview"], bounded["source_preview"],
            "{tool}"
        );
        assert_eq!(clamped["source_preview"]["body_lines"], 500);
        assert_eq!(clamped["source_preview"]["leading_lines"], 20);
        assert_eq!(clamped["source_preview"]["has_more"], true);
        let clamped_low = m
            .payload(
                tool,
                json!({"selector":"app.py:preview","show_source":true,"max_body_lines":0}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(clamped_low["source_preview"]["body_lines"], 1, "{tool}");
    }
    let first_page = m
        .source(
            "app.py:preview",
            json!({"show_source":true,"leading_lines":20,"max_body_lines":501}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(first_page["source_preview"]["body_lines"], 501);
    assert_eq!(first_page["source_preview"]["next_source_offset"], 501);
    assert_eq!(first_page["source_preview"]["has_more"], true);
    let smaller_page = m
        .source(
            "app.py:preview",
            json!({"show_source":true,"leading_lines":20,"max_body_lines":17}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(smaller_page["source_preview"]["body_lines"], 17);
    assert_eq!(smaller_page["source_preview"]["next_source_offset"], 17);
    assert_eq!(smaller_page["source_preview"]["has_more"], true);
    let next_page = m.source("app.py:preview", json!({"show_source":true,"leading_lines":20,"max_body_lines":501,"source_offset":first_page["source_preview"]["next_source_offset"],"generation":first_page["generation"]}),
    ).expect("MCP tool should succeed");
    assert_eq!(next_page["source_preview"]["body_offset"], 501);
    assert_eq!(next_page["source_preview"]["body_lines"], 10);
    assert_eq!(next_page["source_preview"]["has_more"], false);
    m.finish().expect("server must finish successfully");
}

#[test]
fn symbol_boundaries_preserve_source_across_direct_and_mcp_callers() {
    use contextunity_forge_mcp::core::response::{QueryOptions, ResponsePolicy, SourceOptions};
    let cases = [
        (
            "alias.py",
            "from typing import Literal\nStrategy = Literal['direct', 'fallback']; neighbor = 1\n",
            "Strategy",
            "type",
            "Strategy = Literal['direct', 'fallback']",
        ),
        (
            "variable.py",
            "pool_size = 3; neighbor = 1\n",
            "pool_size",
            "variable",
            "pool_size = 3",
        ),
        (
            "fields.py",
            "class Pool:\n    size = (\n        3\n    )\n    def __init__(self):\n        self.strategy = 'direct'\n",
            "size",
            "field",
            "    size = (\n        3\n    )\n",
        ),
        (
            "instance.py",
            "class Pool:\n    def __init__(self):\n        self.strategy = 'direct'\n",
            "strategy",
            "field",
            "        self.strategy = 'direct'\n",
        ),
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
        let (w, harness) = McpTestHarness::indexed_workspace(&[(path, text)]);
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
        let mut m = harness.expect("MCP initialize");
        for tool in ["code_map_inspect", "code_map_explain", "forge_read"] {
            let result = m
                .payload(
                    tool,
                    json!({"selector":id,"show_source":true,"leading_lines":0}),
                )
                .expect("MCP tool should succeed");
            assert_eq!(result["source"], expected, "{path}: {tool}: {result}");
            assert_eq!(result["source_preview"]["start_line"], rows[0]["line"]);
            assert_eq!(result["source_preview"]["end_line"], rows[0]["end_line"]);
        }
        let result = m
            .payload("code_map_search", json!({"pattern":name,"exact":true}))
            .expect("MCP tool should succeed");
        let hit = &result["nodes"]["items"][0];
        assert_eq!(hit["id"], id);
        assert!(hit["source"].as_str().unwrap().contains(expected));
        m.finish().expect("server must finish successfully");
    }
}

#[test]
fn mcp_selector_and_source_boundaries() {
    let (w, m) = McpTestHarness::with_files(&[
        ("scanner.py", "def scanner():\n    return 42\n"),
        (
            "src/app.py",
            "# license\ndef first(): pass\ndef last(): pass\n",
        ),
        ("comments.py", "# license only\n# no code body\n"),
        ("guarded.py", "def guarded(): return 1\n"),
        ("docs/contract.md", "# Contract\n\nA documented contract.\n"),
    ]);
    let mut m = m.expect("MCP initialize");

    for tool in [
        "code_map_inspect",
        "code_map_explain",
        "code_map_impact",
        "code_map_tests",
        "code_map_prove_removal",
        "forge_read",
    ] {
        let rejection = m
            .text(tool, json!({"selector":"scanner"}))
            .expect_err("tool should reject input");
        let expected: &str = "ambiguous selector";
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
        for selector in ["", "   "] {
            let rejection = m
                .text(tool, json!({"selector":selector}))
                .expect_err("tool should reject input");
            assert!(
                rejection
                    .tool_message()
                    .expect("expected MCP tool rejection")
                    .contains("selector is empty"),
                "{tool} {selector:?}: {rejection}"
            );
        }
        let rejection = m
            .text(tool, json!({"selector":"docs/contract.md"}))
            .expect_err("tool should reject input");
        let expected: &str = "get_doc or search_docs";
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    for selector in [
        "scanner.py:scanner",
        "scanner.py::scanner",
        "scanner.py:1",
        "scanner.py#L1",
    ] {
        let result = m
            .payload("code_map_inspect", json!({"selector":selector}))
            .expect("MCP tool should succeed");
        assert_eq!(result["node"]["kind"], "function", "{selector}: {result}");
    }
    for selector in ["file:src/app.py", "file://src/app.py", "./src/app.py"] {
        let result = m
            .payload("code_map_inspect", json!({"selector":selector}))
            .expect("MCP tool should succeed");
        assert_eq!(result["node"]["kind"], "module", "{selector}: {result}");
    }
    for selector in [":::", "#Labc", "nonexistent/path.py:9999", "%.py:scanner"] {
        let rejection = m
            .text("code_map_inspect", json!({"selector":selector}))
            .expect_err("tool should reject input");
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains("selector not found"),
            "{selector}: {rejection}"
        );
    }
    let first = m
        .source("src/app.py:first", json!({"leading_lines":0}))
        .expect("MCP tool should succeed");
    assert_eq!(first["source"], "def first(): pass\n");
    let last = m
        .source("src/app.py:last", json!({"leading_lines":0}))
        .expect("MCP tool should succeed");
    assert_eq!(last["source"], "def last(): pass\n");
    let one_line = m
        .source(
            "src/app.py:first",
            json!({"leading_lines":0,"max_body_lines":0}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(one_line["source"], "def first(): pass\n");
    assert_eq!(one_line["source_preview"]["body_lines"], 1);
    assert_eq!(one_line["source_preview"]["has_more"], false);
    for field in ["leading_lines", "max_body_lines", "source_offset"] {
        let mut arguments = json!({"selector":"src/app.py:first"});
        arguments[field] = json!(-1);
        let rejection = m
            .text("forge_read", arguments)
            .expect_err("tool should reject input");
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains("expected usize"),
            "{field}: {rejection}"
        );
    }
    let comments = m
        .source("comments.py", json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(comments["node"]["kind"], "module");
    assert_eq!(comments["source"], "# license only\n# no code body\n");

    let (_, traversal) = m
        .call(
            "forge_read",
            json!({"selector":"scanner.py:scanner","path":"../outside.py","format":"json"}),
        )
        .expect("MCP transport");
    assert_eq!(traversal["result"]["isError"], true, "{traversal}");
    #[cfg(unix)]
    {
        let outside =
            Workspace::with_files(&[("outside.py", "def guarded(): return 'must-not-leak'\n")]);
        w.remove("guarded.py");
        std::os::unix::fs::symlink(outside.path("outside.py"), w.path("guarded.py")).unwrap();
        let (_, response) = m
            .call(
                "forge_read",
                json!({"selector":"guarded.py:guarded","leading_lines":0,"format":"json"}),
            )
            .expect("MCP transport");
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            !response.to_string().contains("must-not-leak"),
            "{response}"
        );
    }
    m.finish().expect("server must finish successfully");
}

#[test]
fn removal_proof_normalizes_file_selector_aliases_before_scoping() {
    let mut m = McpTestHarness::indexed(&[
        (
            "target.py",
            "def target(): return missing_target()\ndef spare(): return 1\n",
        ),
        (
            "caller_a.py",
            "from target import target\ndef caller_a(): return target()\n",
        ),
        (
            "caller_b.py",
            "from target import target\ndef caller_b(): return target()\n",
        ),
    ])
    .expect("MCP initialize");
    let full = m
        .payload(
            "code_map_prove_removal",
            json!({"selector":"target.py","limit":100}),
        )
        .expect("MCP tool should succeed");
    let expected_selected = crate::json_array(&full["selected_ids"]["items"]).clone();
    let expected_dependencies = crate::json_array(&full["incoming_dependencies"]["items"]).clone();
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
        let first = m
            .payload(
                "code_map_prove_removal",
                json!({"selector":selector,"limit":1,"offset":0}),
            )
            .expect("MCP tool should succeed");
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
                m.payload(
                    "code_map_prove_removal",
                    json!({"selector":selector,"limit":1,"offset":offset,"generation":generation}),
                )
                .expect("MCP tool should succeed")
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
                crate::json_array(&result["selected_ids"]["items"])
                    .iter()
                    .cloned(),
            );
            dependency_items.extend(
                crate::json_array(&result["incoming_dependencies"]["items"])
                    .iter()
                    .cloned(),
            );
        }
        assert_eq!(selected_items, expected_selected, "{selector}");
        assert_eq!(dependency_items, expected_dependencies, "{selector}");
    }
    m.finish().expect("server must finish successfully");
}
