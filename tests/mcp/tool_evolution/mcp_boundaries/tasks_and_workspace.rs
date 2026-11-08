use super::*;

#[test]
fn ergonomic_task_aliases_reach_existing_operations() {
    let w = Workspace::with_files(&[
        (
            "forge-mcp.yaml",
            "roots: [.]\ndocs: [docs]\nmilestones: [docs/milestones]\n",
        ),
        ("app.py", "def first(): pass\n"),
        (
            "docs/milestones/010-aliases.md",
            "---\nid: m-aliases\ntitle: Parameter aliases\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: aliases\ntarget: Accept parameter aliases\nproof_policy: seam-test-first\nscope: [app.py]\nsubtasks:\n  - subtask_ref: parameters\n    title: Accept alternate parameter names\n```\n",
        ),
    ]);

    w.build();
    w.query(&["task", "sync", "--file", "docs/milestones/010-aliases.md"])
        .expect("CLI should succeed");
    let task_id = "forge-mcp/m-aliases:aliases";
    let inspected = w
        .query(&["task", "inspect", task_id])
        .expect("CLI should succeed");
    for args in [
        vec!["task", "show", task_id],
        vec!["task", "inspect", "--task", task_id],
        vec!["task", "show", "--task", task_id],
    ] {
        assert_eq!(w.query(&args).expect("CLI should succeed"), inspected);
    }
    let compact = w
        .query(&["task", "list", "--milestone", "010", "--detail", "compact"])
        .expect("CLI should succeed");
    assert_eq!(crate::json_array(&compact["tasks"]).len(), 1);
    let full = w
        .query(&["task", "list", "--milestone", "010", "--full"])
        .expect("CLI should succeed");
    assert_eq!(
        w.query(&["task", "list", "--milestone", "010", "--detail", "full"])
            .expect("CLI should succeed"),
        full
    );
    let mut m = McpTestHarness::new(&w).expect("MCP initialize");
    let canonical = m
        .payload("task_list", json!({"milestone_ref":"010","detail":"full"}))
        .expect("MCP tool should succeed");
    assert_eq!(
        m.payload("task_list", json!({"milestone":"010","detail":"full"}))
            .expect("MCP tool should succeed"),
        canonical
    );
    assert_eq!(crate::json_array(&canonical["tasks"]).len(), 1);
    let first_page = m
        .payload(
            "task_list",
            json!({"milestone_ref":"010","status":"all","limit":1,"offset":0}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(first_page["pagination"]["total"], 1);
    assert_eq!(first_page["pagination"]["limit"], 1);
    assert_eq!(first_page["pagination"]["returned"], 1);
    assert_eq!(first_page["pagination"]["has_more"], false);
    assert!(!first_page["pagination"]["generation"]
        .as_str()
        .unwrap()
        .is_empty());
    assert!(first_page["pagination"]["continuation_hint"]
        .to_string()
        .contains("010"));
    let cli_page = w
        .query(&[
            "task",
            "list",
            "--milestone",
            "010",
            "--status",
            "all",
            "--limit",
            "1",
            "--offset",
            "0",
        ])
        .expect("CLI should succeed");
    assert_eq!(cli_page["pagination"], first_page["pagination"]);
    assert_eq!(cli_page["tasks"], first_page["tasks"]);
    let final_page = m
        .payload(
            "task_list",
            json!({
                "milestone_ref":"010",
                "status":"all",
                "limit":1,
                "offset":1,
                "generation":first_page["pagination"]["generation"]
            }),
        )
        .expect("MCP tool should succeed");
    assert!(crate::json_array(&final_page["tasks"]).is_empty());
    assert!(final_page["pagination"].get("next_offset").is_none());
    assert_eq!(final_page["pagination"]["has_more"], false);
    let flags = [
        "--stage",
        "contract",
        "--action",
        "pass",
        "--evidence",
        "{}",
    ];
    let submit = |selector: &[&str]| {
        let mut args = vec!["task", "submit"];
        args.extend_from_slice(selector);
        args.extend(flags);
        run_cli(&w, &args)
    };
    let positional = submit(&[task_id]);
    let flagged = submit(&["--task", task_id]);
    assert!(!positional.status.success());
    assert_eq!(flagged.status.code(), positional.status.code());
    assert_eq!(flagged.stderr, positional.stderr);
    for args in [
        vec!["task", "inspect"],
        vec!["task", "inspect", task_id, "--task", task_id],
        [
            vec!["task", "submit", task_id, "--task", task_id],
            flags.to_vec(),
        ]
        .concat(),
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
    m.finish().expect("server must finish successfully");
}

#[test]
fn guide_topics_preserve_query_and_acdd_shapes_and_reject_unknown_topics() {
    let mut m = McpTestHarness::indexed(&[]).expect("MCP initialize");
    let guide = m
        .payload("forge_guide", json!({"topic":"query"}))
        .expect("MCP tool should succeed");
    assert!(guide["start"].is_string());
    let acdd = m
        .payload("forge_guide", json!({"topic":"acdd"}))
        .expect("MCP tool should succeed");
    assert!(acdd["gates"].is_array());
    assert!(acdd["proof_policies"].is_object());
    let rejection = m
        .text("forge_guide", json!({"topic":"no-such-topic"}))
        .expect_err("tool should reject input");
    let expected: &str = "unknown guide topic";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    m.finish().expect("server must finish successfully");
}

#[test]
fn granular_diagnostics_preserve_resolution_evidence_and_crlf_utf8_source() {
    let diagnostics = DIAGNOSTICS_SOURCE;
    let fixture = AnalysisFixture::from_workspace(Workspace::with_files(&[
        ("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n"),
        ("cycle.py", "def a(): b()\ndef b(): a()\n"),
        ("other.py", "def idle(): pass\n"),
        ("requirements.txt", "pydantic\nrequests\n"),
        ("diagnostics.py", diagnostics),
    ]));
    let w = fixture.workspace();
    w.build();
    let mut m = fixture.start_mcp().expect("MCP initialize");
    let mut diagnostic_failures = Vec::new();
    let mut collect_pages = |mut arguments: serde_json::Value, field: &str| {
        arguments["offset"] = json!(0);
        arguments["generation"] = serde_json::Value::Null;
        let mut items = Vec::new();
        loop {
            let result = m
                .payload("code_map_analyze", arguments.clone())
                .expect("MCP tool should succeed");
            let page = &result[field];
            let page_items = page["items"]
                .as_array()
                .expect("diagnostic page should include items");
            items.extend(page_items.iter().cloned());
            if page["has_more"] != true {
                break;
            }
            arguments["offset"] = json!(page["next_offset"]
                .as_u64()
                .expect("continued diagnostic page should include next offset"));
            arguments["generation"] = page["generation"].clone();
        }
        items
    };
    for detail in ["compact", "full"] {
        let arguments = json!({"target":"diagnostics.py","detail":detail,"limit":1});
        let items = collect_pages(arguments.clone(), "resolution");
        let external = collect_pages(arguments, "external_imports");
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
            assert_eq!(
                item["status"], "unresolved",
                "{detail}/{expression}: {item}"
            );
            assert!(item["cause"].is_string(), "{detail}/{expression}: {item}");
            if detail == "full" {
                assert!(
                    item["evidence"].is_string(),
                    "{detail}/{expression}: {item}"
                );
            }
            if !item["reason"].is_null() {
                diagnostic_failures.push(format!(
                    "{detail}/{expression}: unproven category must stay null, actual={item}"
                ));
            }
        }
    }
    let conn = w.open();
    let phantom_edges: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE p.path='diagnostics.py' AND e.line IN(7,9,11,13,15) AND e.kind IN('calls','possible_calls')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        phantom_edges, 0,
        "diagnostic labels cannot create graph targets"
    );
    drop(conn);

    assert!(
        diagnostic_failures.is_empty(),
        "{}",
        diagnostic_failures.join("\n")
    );
    m.finish().expect("server must finish successfully");
}

#[test]
fn workspace_coverage_keeps_published_scope_identity_across_restart_and_rebuild() {
    let workspace = Workspace::with_files(&[
        ("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n"),
        ("cycle.py", "def a(): b()\ndef b(): a()\n"),
        ("other.py", "def idle(): pass\n"),
        ("requirements.txt", "pydantic\nrequests\n"),
        ("diagnostics.py", DIAGNOSTICS_SOURCE),
    ]);
    let fixture = AnalysisFixture::from_workspace(workspace);
    let w = fixture.workspace();
    let mut m = fixture.start_mcp().expect("MCP initialize");
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
    // Scope identity and counts come from this published snapshot, not from
    // component labels or the current adapter after it has changed.
    let full = m
        .payload("code_map_overview", json!({"limit":100}))
        .expect("MCP tool should succeed");
    let scopes = full["coverage_by_workspace"]["items"]
        .as_array()
        .expect("persisted workspace breakdown");
    assert_eq!(full["coverage_by_workspace"]["total"], 2);
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
        let prefix_parameter: &dyn rusqlite::ToSql = &prefix;
        let expected = fixture
            .rows(
                "SELECT c.status,count(*) total FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE (?1='' AND p.path NOT LIKE '[linked]/%') OR (?1<>'' AND substr(p.path,1,length(?1))=?1) GROUP BY c.status",
                &[prefix_parameter],
                10,
            )
            .expect("read persisted coverage status counts");
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
    let first = m
        .payload(
            "code_map_overview",
            json!({"aspects":["coverage_by_workspace"],"limit":1}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(first["coverage_by_workspace"]["has_more"], true);
    let next = m.payload("code_map_overview", json!({"aspects":["coverage_by_workspace"],"limit":1,"offset":first["coverage_by_workspace"]["next_offset"],"generation":first["coverage_by_workspace"]["generation"]})).expect("MCP tool should succeed");
    let mut paged = vec![
        first["coverage_by_workspace"]["items"][0].clone(),
        next["coverage_by_workspace"]["items"][0].clone(),
    ];
    paged.sort_by_key(|scope| scope["name"].as_str().unwrap().to_owned());
    let mut expected = scopes.clone();
    expected.sort_by_key(|scope| scope["name"].as_str().unwrap().to_owned());
    assert_eq!(paged, expected);
    m.finish().expect("server must finish before restart");
    let mut restarted = fixture.start_mcp().expect("MCP initialize");
    assert_eq!(
        restarted
            .payload("code_map_overview", json!({"limit":100}))
            .expect("MCP tool should succeed")["coverage_by_workspace"],
        full["coverage_by_workspace"]
    );
    restarted
        .finish()
        .expect("server must finish before restart");
    w.write("forge-mcp.yaml", "roots: [.]\n");
    let connection = w.open();
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .expect("resolve persisted workspace coverage page");
    let persisted = reader::overview_with_options(
        &connection,
        &reader::OverviewOptions {
            aspects: None,
            page: &page,
        },
    )
    .expect("read published workspace coverage before rebuild");
    drop(connection);
    assert_eq!(
        persisted["coverage_by_workspace"]["items"],
        full["coverage_by_workspace"]["items"]
    );
    assert_eq!(
        persisted["coverage_by_workspace"]["total"],
        full["coverage_by_workspace"]["total"]
    );
    w.build();
    let mut rebuilt = fixture.start_mcp().expect("MCP initialize");
    let changed = rebuilt
        .payload("code_map_overview", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert_eq!(changed["coverage_by_workspace"]["total"], 1);
    assert_eq!(
        changed["coverage_by_workspace"]["items"][0]["name"],
        "primary"
    );
    assert!(full.get("generation").is_none());
    assert!(changed.get("generation").is_none());
    rebuilt.finish().expect("server must finish successfully");
}
