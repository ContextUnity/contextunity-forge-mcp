use super::*;

#[test]
fn compact_inspect_and_explain_omit_repeated_empty_data_without_losing_edges() {
    let workspace = Workspace::new();
    workspace.write("graph.py", "def target(): pass\ndef caller(): target()\n");
    let conn = workspace.build();
    let compact = reader::inspect_paged(&conn, "target", false, &options(10)).unwrap();
    assert_eq!(compact["documents"], serde_json::json!({"total": 0}));
    let incoming = reader::explain_with_options(
        &conn,
        "target",
        &reader::ExplainOptions {
            direction: Some("incoming"),
            show_doc: true,
            page: &options(10),
        },
    )
    .unwrap();
    let edge = &incoming["incoming"]["items"][0];
    assert_eq!(edge["kind"], "calls");
    assert!(edge["src_public_id"].is_string());
    assert!(edge.get("dst_public_id").is_none());
    assert!(incoming["incoming"]["generation"].is_string());
    let mut full = options(10);
    full.detail = Detail::Full;
    let full_edge = reader::explain_with_options(
        &conn,
        "target",
        &reader::ExplainOptions {
            direction: Some("incoming"),
            show_doc: true,
            page: &full,
        },
    )
    .unwrap();
    assert_eq!(
        full_edge["incoming"]["items"][0]["dst_public_id"],
        full_edge["node"]["id"]
    );
}

#[test]
fn compact_symbol_summary_and_coverage_are_bounded_and_opt_in() {
    let workspace = Workspace::new();
    let mut code = String::from("def target(value: int):\n    \"\"\"Target documentation.\"\"\"\n");
    for i in 0..7 {
        code.push_str(&format!("    helper_{i:02}()\n"));
    }
    code.push_str("    missing_dependency()\n\n");
    for i in 0..7 {
        code.push_str(&format!("def helper_{i:02}(): return {i}\n"));
        code.push_str(&format!("def caller_{i:02}(): return target({i})\n"));
    }
    workspace.write("graph.py", &code);
    let conn = workspace.build();
    let source = SourceOptions::resolve(&ResponsePolicy::default(), None, None, None, 0).unwrap();

    let inspected = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    let summary = &inspected["summary"];
    assert_eq!(summary["signature"], "def target(value: int):");
    assert_eq!(summary["docstring"], "\"\"\"Target documentation.\"\"\"");
    assert_eq!(summary["container"]["kind"], "module");
    assert_eq!(summary["calls"]["inbound"]["count"], 7);
    assert_eq!(summary["calls"]["inbound"]["occurrences"], 7);
    assert_eq!(summary["calls"]["outbound"]["count"], 7);
    assert_eq!(summary["calls"]["outbound"]["occurrences"], 7);
    assert_eq!(
        summary["calls"]["inbound"]["preview"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        summary["calls"]["outbound"]["preview"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        summary["calls"]["inbound"]["preview"][0]["name"],
        "caller_00"
    );
    assert_eq!(
        summary["calls"]["outbound"]["preview"][0]["name"],
        "helper_00"
    );
    assert!(inspected.get("coverage").is_none());

    let explained = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::ExplainOptions {
            direction: None,
            show_doc: true,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    assert_eq!(explained["summary"], inspected["summary"]);
    assert!(explained.get("coverage").is_none());

    let with_coverage = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions {
                include_coverage: true,
            },
            page: &options(20),
        },
    )
    .unwrap();
    assert!(with_coverage["coverage"]["total"].as_u64().unwrap() >= 1);
    assert_eq!(with_coverage["summary"], inspected["summary"]);
}

#[test]
fn compact_summary_preserves_the_exact_nested_python_container() {
    let workspace = Workspace::new();
    workspace.write(
        "nested.py",
        "class A:\n    class Item:\n        def handle(self): return 1\n\nclass B:\n    class Item:\n        def handle(self): return 2\n",
    );
    let conn = workspace.build();
    let (method_id, receiver_type): (String, String) = conn
        .query_row(
            "SELECT n.id,json_extract(n.details,'$.receiver_type') FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='nested.py' AND n.name='handle' AND n.kind='method' ORDER BY n.line DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(receiver_type, "Item");
    let (expected_id, expected_qualname, expected_line): (String, String, usize) = conn
        .query_row(
            "SELECT parent.id,parent.qualname,parent.line FROM edges e JOIN nodes parent ON parent.node_hash=e.src_hash WHERE e.dst_hash=(SELECT node_hash FROM nodes WHERE id=?1) AND e.kind='contains' AND parent.kind='class'",
            [&method_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(expected_qualname, "nested.B.Item");

    let source = SourceOptions::resolve(&ResponsePolicy::default(), None, None, None, 0).unwrap();
    let inspected = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        &method_id,
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(inspected["summary"]["container"]["id"], expected_id);
    assert_eq!(
        inspected["summary"]["container"]["qualname"],
        expected_qualname
    );
    assert_eq!(inspected["summary"]["container"]["line"], expected_line);
}

#[test]
fn compact_summary_call_counts_obey_vm_budget_and_reset_connection() {
    let workspace = Workspace::new();
    workspace.write("graph.py", "def target(): pass\n");
    drop(workspace.build());

    let conn = Connection::open(workspace.db()).unwrap();
    let target_id: String = conn
        .query_row(
            "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='graph.py' AND n.name='target'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let target_hash: i64 = conn
        .query_row(
            "SELECT node_hash FROM nodes WHERE id=?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    let path_id: i64 = conn
        .query_row(
            "SELECT path_id FROM path_dictionary WHERE path='graph.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // A bounded virtual fan-out forces the normal inspect path through its SQLite VM budget.
    conn.execute_batch(&format!(
        "CREATE TEMP VIEW edges AS
         WITH RECURSIVE fanout(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM fanout WHERE n<10000000)
         SELECT * FROM main.edges WHERE kind<>'calls'
         UNION ALL
         SELECT {target_hash},'calls',{target_hash},{path_id},1,1,1,1 FROM fanout;
         PRAGMA query_only=ON;"
    ))
    .unwrap();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(false), None, None, 0).unwrap();
    let started = std::time::Instant::now();
    let error = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        &target_id,
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap_err();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the aggregate count should stop at QueryBudget, elapsed {:?}",
        started.elapsed()
    );
    assert!(
        error.to_string().contains("interrupt"),
        "expected the call-count aggregate to be interrupted by QueryBudget, got: {error}"
    );

    let next_query: i64 = conn
        .query_row(
            "WITH RECURSIVE check_rows(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM check_rows WHERE n<10000) SELECT max(n) FROM check_rows",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(next_query, 10_000);
}

#[test]
fn query_sql_operation_pages_secure_selects_and_validates_impact_direction() {
    let workspace = Workspace::new();
    let mut code = (0..21)
        .map(|i| format!("def f{i:02}(): return {i}\n"))
        .collect::<String>();
    code.push_str(
        "def helper(): return 1\ndef target(): return helper()\ndef caller(): return target()\n",
    );
    workspace.write("graph.py", &code);
    let conn = workspace.build();
    let sql = "SELECT name FROM nodes WHERE kind='function' AND name LIKE 'f%' ORDER BY name";
    let mut query = options(6);
    let mut names = Vec::new();
    loop {
        let result = traversal::query_with_options(
            &conn,
            "sql",
            Some(sql),
            &traversal::GraphQueryOptions {
                depth: 1,
                direction: None,
                coverage: CoverageOptions::default(),
                page: &query,
            },
        )
        .unwrap();
        let page = &result["rows"];
        assert_eq!(page["total"], 21);
        names.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["name"].as_str().unwrap().to_owned()),
        );
        if !continue_page(&mut query, page) {
            break;
        }
    }
    assert_eq!(
        names,
        (0..21).map(|i| format!("f{i:02}")).collect::<Vec<_>>()
    );
    assert!(traversal::query_with_options(
        &conn,
        "sql",
        Some("UPDATE nodes SET kind='changed'"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(6)
        }
    )
    .is_err());
    assert!(traversal::query_with_options(
        &conn,
        "sql",
        None,
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(6)
        }
    )
    .is_err());

    let inbound = traversal::query_with_options(
        &conn,
        "impact",
        Some("target"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(inbound["direction"], "inbound");
    assert!(inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "caller"));
    let outbound = traversal::query_with_options(
        &conn,
        "impact",
        Some("target"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: Some("outbound"),
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(outbound["direction"], "outbound");
    assert!(outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "helper"));
    assert!(traversal::query_with_options(
        &conn,
        "impact",
        Some("target"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: Some("sideways"),
            coverage: CoverageOptions::default(),
            page: &options(10)
        }
    )
    .is_err());
}

#[test]
fn document_excerpt_is_opt_in_and_bounded_around_match() {
    let workspace = Workspace::new();
    workspace.write(
        "docs/guide.md",
        &format!(
            "# Guide\n\n{} needle {}\n",
            "before ".repeat(80),
            "after ".repeat(80)
        ),
    );
    let conn = workspace.build();
    let basic = reader::search_docs_with_options(
        &conn,
        "needle",
        &reader::DocSearchOptions {
            doc_type: None,
            component: None,
            include_excerpt: false,
            page: &options(10),
        },
    )
    .unwrap();
    assert!(basic["sections"]["items"][0].get("excerpt").is_none());
    let with_excerpt = reader::search_docs_with_options(
        &conn,
        "needle",
        &reader::DocSearchOptions {
            doc_type: None,
            component: None,
            include_excerpt: true,
            page: &options(10),
        },
    )
    .unwrap();
    let excerpt = with_excerpt["sections"]["items"][0]["excerpt"]
        .as_str()
        .unwrap();
    assert!(excerpt.contains("[needle]"), "{excerpt}");
    assert!(excerpt.chars().count() <= 240);
    assert_eq!(
        basic["sections"]["total"],
        with_excerpt["sections"]["total"]
    );
    assert_eq!(
        basic["sections"]["generation"],
        with_excerpt["sections"]["generation"]
    );
}

#[test]
fn document_excerpt_uses_the_matching_fts_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT);\
         INSERT INTO metadata VALUES('output_root','test-generation');\
         CREATE TABLE doc_sections(doc_id TEXT PRIMARY KEY,path TEXT,section_title TEXT,doc_type TEXT,content TEXT,invariants TEXT,size INTEGER,is_invariant INTEGER);\
         CREATE VIRTUAL TABLE doc_search USING fts5(section_title,content,invariants,content='doc_sections',content_rowid='rowid');\
         INSERT INTO doc_sections VALUES('doc:1','docs/one.md','titlematch','guide','ordinary body','rulematch',13,1);\
         INSERT INTO doc_search(rowid,section_title,content,invariants) SELECT rowid,section_title,content,invariants FROM doc_sections;",
    ).unwrap();
    for query in ["titlematch", "rulematch"] {
        let result = reader::search_docs_with_options(
            &conn,
            query,
            &reader::DocSearchOptions {
                doc_type: None,
                component: None,
                include_excerpt: true,
                page: &options(10),
            },
        )
        .unwrap();
        assert_eq!(result["sections"]["total"], 1);
        let excerpt = result["sections"]["items"][0]["excerpt"].as_str().unwrap();
        assert!(
            excerpt.contains(&format!("[{query}]")),
            "{query}: {excerpt}"
        );
    }
}

#[test]
fn search_path_scopes_file_and_descendants_without_changing_page_contract() {
    let workspace = Workspace::new();
    workspace.write("pkg/a.py", "def lookup(): pass\n");
    workspace.write("pkg/sub/b.py", "def lookup(): pass\n");
    workspace.write("other.py", "def lookup(): pass\n");
    let conn = workspace.build();
    let all = symbols::search_with_options(
        &conn,
        "lookup",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(all["nodes"]["total"], 3);
    let directory = symbols::search_with_options(
        &conn,
        "lookup",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: Some("./pkg"),
            include_docs: false,
            exact: false,
            page: &options(1),
        },
    )
    .unwrap();
    assert_eq!(directory["nodes"]["total"], 2);
    assert_eq!(directory["nodes"]["items"][0]["path"], "pkg/a.py");
    assert!(directory["nodes"]["has_more"].as_bool().unwrap());
    let file = symbols::search_with_options(
        &conn,
        "lookup",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: Some("file:pkg/sub/b.py"),
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(file["nodes"]["total"], 1);
    assert_eq!(file["nodes"]["items"][0]["path"], "pkg/sub/b.py");
    for invalid in ["", "../other.py", "/tmp/other.py", "pkg//sub", "pkg\\sub"] {
        assert!(
            symbols::search_with_options(
                &conn,
                "lookup",
                &symbols::SearchOptions {
                    kind: None,
                    path: Some(invalid),
                    include_docs: false,
                    exact: false,
                    page: &options(10)
                }
            )
            .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn line_anchor_prefers_exact_path_and_fails_closed_for_ambiguous_suffixes() {
    let workspace = Workspace::new();
    workspace.write("alpha/shared.py", "def target(): return 'alpha'\n");
    workspace.write("beta/shared.py", "def target(): return 'beta'\n");
    workspace.write("nested/alpha/shared.py", "def target(): return 'nested'\n");
    let conn = workspace.build();

    let exact = reader::select(&conn, "alpha/shared.py#L1").unwrap();
    assert_eq!(exact["path"], "alpha/shared.py");

    let error = reader::select(&conn, "shared.py#L1").unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<reader::SelectorError>(),
            Some(reader::SelectorError::Ambiguous { .. })
        ),
        "expected typed ambiguity for multiple whole-path suffix matches, got: {error}"
    );
}

#[test]
fn line_anchor_does_not_match_a_partial_filename_component() {
    let workspace = Workspace::new();
    workspace.write("notshared.py", "def target(): return 'wrong file'\n");
    let conn = workspace.build();

    let error = reader::select(&conn, "shared.py#L1").unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<reader::SelectorError>(),
            Some(reader::SelectorError::NotFound { .. })
        ),
        "expected line anchor to reject a partial filename suffix, got: {error}"
    );
}

#[test]
fn line_anchor_does_not_fall_back_when_exact_file_has_no_node_on_the_line() {
    let workspace = Workspace::new();
    workspace.write("alpha/shared.py", "def target(): return 'exact'\n");
    workspace.write(
        "nested/alpha/shared.py",
        "def target():\n    return 'nested'\n",
    );
    let conn = workspace.build();

    let error = reader::select(&conn, "alpha/shared.py#L2").unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<reader::SelectorError>(),
            Some(reader::SelectorError::NotFound { .. })
        ),
        "expected the exact file's missing line to remain not found, got: {error}"
    );
}

#[test]
fn selector_propagates_candidate_enrichment_row_budget_errors() {
    let workspace = Workspace::new();
    workspace.write("alpha.py", "def repeated(): return 'alpha'\n");
    workspace.write("beta.py", "def repeated(): return 'beta'\n");
    drop(workspace.build());

    // NULs fit under SQLite's per-cell limit but expand to more than the JSON row budget.
    let oversized_details = "\0".repeat(1_500_000);
    let conn = Connection::open(workspace.db()).unwrap();
    conn
        .execute(
            "UPDATE nodes SET details=?1 WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='alpha.py') AND name='repeated'",
            [&oversized_details],
        )
        .unwrap();

    let error = reader::select(&conn, "repeated").unwrap_err();
    assert!(
        error.to_string().contains("8 MiB row budget"),
        "expected the candidate enrichment budget error to propagate, got: {error}"
    );
}
