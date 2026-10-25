use super::*;

#[test]
fn compact_inspect_and_explain_omit_repeated_empty_data_without_losing_edges() {
    let workspace = Workspace::new();
    workspace.write(
        "graph.py",
        "def target(): pass\ndef caller(): target(); target()\n",
    );
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
    assert_eq!(edge["dst_public_id"], incoming["node"]["id"]);
    assert_eq!(edge["direction"], "inbound");
    assert_eq!(edge["path"], "graph.py");
    assert_eq!(edge["line"], 2);
    assert_eq!(edge["occurrence_count"], 2);
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
    workspace.write(
        "docs/guide.md",
        "---\ntitle: Inspect References\ndoc_type: guide\n---\n# Inspect References\n## Target overview\nDistinct full-detail inspect content.\n[`target`](../graph.py#L1)\n## Target consumers\n[`target`](../graph.py#L1)\n",
    );
    workspace.write("overview.py", "def code_map_overview(): pass\n");
    workspace.write(
        "ambiguous.py",
        "def shared(): return 'first'\ndef shared(): return 'second'\n",
    );
    workspace.write(
        "docs/reference/mcp-tools.md",
        "---\ntitle: MCP tools\ndoc_type: reference\n---\n# MCP tools\n## Overview one\n[`code_map_overview`](../../overview.py#L1)\n\n## Overview two\n[`code_map_overview`](../../overview.py#L1)\n\n## Overview three\n[`code_map_overview`](../../overview.py#L1)\n\n## Overview four\n[`code_map_overview`](../../overview.py#L1)\n\n## Ambiguous candidates\n[`shared`](../../ambiguous.py#L1)\n",
    );
    let conn = workspace.build();
    let source = SourceOptions::resolve(&ResponsePolicy::default(), None, None, None, 0).unwrap();

    let mut full_doc_options = options(20);
    full_doc_options.detail = Detail::Full;
    let document = reader::get_doc_paged(
        &conn,
        "docs/reference/mcp-tools.md",
        None,
        &full_doc_options,
    )
    .unwrap();
    let references = document["referenced_symbols"]["items"]
        .as_array()
        .expect("verified code references");
    assert_eq!(references.len(), 4);
    assert!(references
        .iter()
        .all(|reference| reference["name"] == "code_map_overview"));
    assert!(references.iter().all(|reference| reference["line"] == 1));
    let candidates = document["referenced_symbol_candidates"]["items"]
        .as_array()
        .expect("full-detail candidate rows");
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|candidate| {
        candidate["source"]["path"] == "docs/reference/mcp-tools.md"
            && candidate["source"]["name"] == "shared"
            && candidate["source"]["line"].as_u64().is_some()
            && candidate["target"]["path"] == "ambiguous.py"
    }));

    let ambiguous_doc_id = document["sections"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["section_title"] == "Ambiguous candidates")
        .expect("ambiguous source section")["doc_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fallback_conn = Connection::open(workspace.db()).unwrap();
    fallback_conn
        .execute(
            "UPDATE nodes SET details='{}' WHERE id=?1",
            [&ambiguous_doc_id],
        )
        .unwrap();
    let fallback_document = reader::get_doc_paged(
        &fallback_conn,
        "docs/reference/mcp-tools.md",
        None,
        &full_doc_options,
    )
    .unwrap();
    assert_eq!(
        fallback_document["referenced_symbol_candidates"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "legacy symbol references remain structurally parsed when detail claims are absent"
    );
    let citation_identities = references
        .iter()
        .map(|reference| {
            (
                reference["doc_id"]
                    .as_str()
                    .expect("citing section id")
                    .to_owned(),
                reference["citation_line"].as_u64().expect("citation line"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        citation_identities
            .iter()
            .map(|(doc_id, _)| doc_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "each citation keeps its source section"
    );
    assert_eq!(
        citation_identities
            .iter()
            .map(|(_, citation_line)| citation_line)
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "each citation keeps its source line independently of target coordinates"
    );
    assert_eq!(
        citation_identities
            .iter()
            .map(|(_, citation_line)| *citation_line)
            .collect::<std::collections::BTreeSet<_>>(),
        [7, 10, 13, 16].into_iter().collect()
    );
    let mut compact_doc_options = options(20);
    compact_doc_options.detail = Detail::Compact;
    let compact_document = reader::get_doc_paged(
        &conn,
        "docs/reference/mcp-tools.md",
        None,
        &compact_doc_options,
    )
    .unwrap();
    let compact_references = compact_document["referenced_symbols"]["items"]
        .as_array()
        .unwrap();
    assert!(compact_references
        .iter()
        .all(|reference| reference["line"] == 1));
    assert!(compact_references.iter().all(|reference| {
        reference["citation_line"].as_u64().is_some() && reference["doc_id"].as_str().is_some()
    }));
    assert_eq!(
        compact_document["referenced_symbol_candidates"]["omitted"],
        true
    );
    let mut citation_page_options = options(1);
    let mut paged_citations = Vec::new();
    loop {
        let page = reader::get_doc_paged(
            &conn,
            "docs/reference/mcp-tools.md",
            None,
            &citation_page_options,
        )
        .unwrap();
        paged_citations.extend(
            page["referenced_symbols"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|reference| {
                    assert_eq!(reference["line"], 1);
                    (
                        reference["doc_id"].as_str().unwrap().to_owned(),
                        reference["citation_line"].as_u64().unwrap(),
                    )
                }),
        );
        if !continue_page(&mut citation_page_options, &page["referenced_symbols"]) {
            break;
        }
    }
    assert_eq!(paged_citations, citation_identities);

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
    assert_eq!(explained["documents"]["total"], 2);
    assert!(explained.get("documented_in").is_none());

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

    for operation in ["code_map_inspect", "code_map_query"] {
        let mut page = options(1);
        let inspect = |page: &QueryOptions| match operation {
            "code_map_inspect" => symbols::inspect_with_options(
                &conn,
                &workspace.0,
                "target",
                &symbols::InspectOptions {
                    show_doc: true,
                    source: &source,
                    coverage: CoverageOptions {
                        include_coverage: true,
                    },
                    page,
                },
            ),
            "code_map_query" => traversal::query_with_options(
                &conn,
                "inspect",
                Some("target"),
                &traversal::GraphQueryOptions {
                    depth: 1,
                    direction: None,
                    coverage: CoverageOptions {
                        include_coverage: true,
                    },
                    page,
                },
            ),
            _ => unreachable!(),
        };
        let first = inspect(&page).unwrap();
        assert_eq!(first["documents"]["total"], 2, "{operation} JSON page");
        let documented_in = &first["documented_in"];
        assert_eq!(documented_in["total"], 2, "{operation}");
        assert_eq!(documented_in["limit"], 1, "{operation}");
        assert_eq!(documented_in["has_more"], true, "{operation}");
        assert!(
            documented_in["generation"].as_str().is_some(),
            "{operation}"
        );
        let first_citation = &documented_in["items"][0];
        assert!(first_citation["doc_id"].as_str().is_some(), "{operation}");
        assert_eq!(first_citation["path"], "docs/guide.md", "{operation}");
        assert!(
            ["Target overview", "Target consumers"]
                .contains(&first_citation["section_title"].as_str().unwrap()),
            "{operation}: {first_citation}"
        );
        assert_eq!(first_citation["doc_type"], "guide", "{operation}");
        assert!(
            first_citation["line"].as_u64().unwrap_or_default() > 0,
            "{operation}"
        );
        assert!(
            first_citation["doc_line"].as_u64().unwrap_or_default() > 0,
            "{operation}"
        );
        assert_eq!(first["coverage"]["limit"], 1, "{operation}");
        assert!(
            first["coverage"]["has_more"].as_bool().unwrap_or(false),
            "{operation}"
        );
        assert!(
            first["coverage"]["generation"].as_str().is_some(),
            "{operation}"
        );
        let coverage_item = &first["coverage"]["items"][0];
        assert!(
            coverage_item["expression"].as_str().is_some(),
            "{operation}"
        );
        assert!(coverage_item.get("doc_id").is_none(), "{operation}");

        let title = operation;
        let discovery = Some(contextunity_forge_mcp::core::format::DiscoveryKind::Inspect);
        let markdown = contextunity_forge_mcp::core::format::render_for_operation(
            &first,
            contextunity_forge_mcp::core::format::OutputFormat::Markdown,
            title,
            discovery,
            contextunity_forge_mcp::core::format::OperationKind::Navigation,
        );
        assert!(
            !markdown.contains("## documents\n\n```json"),
            "{operation} emitted the parallel documents JSON page: {markdown}"
        );

        let mut full_page = options(1);
        full_page.detail = Detail::Full;
        let full_inspect = inspect(&full_page).unwrap();
        let full_markdown = contextunity_forge_mcp::core::format::render_for_operation(
            &full_inspect,
            contextunity_forge_mcp::core::format::OutputFormat::Markdown,
            operation,
            discovery,
            contextunity_forge_mcp::core::format::OperationKind::Navigation,
        );
        assert!(
            full_markdown.contains("Distinct full-detail inspect content."),
            "{operation} dropped full document content: {full_markdown}"
        );
        assert!(
            full_markdown.contains(first_citation["doc_id"].as_str().unwrap())
                && full_markdown.contains("type `guide`")
                && full_markdown.contains(&format!(
                    "citation line {}",
                    first_citation["line"].as_u64().unwrap()
                )),
            "{operation} dropped documented_in metadata: {full_markdown}"
        );

        assert!(continue_page(&mut page, documented_in), "{operation}");
        let second = inspect(&page).unwrap();
        let second_citation = &second["documented_in"]["items"][0];
        assert_eq!(second["documented_in"]["has_more"], false, "{operation}");
        assert_ne!(
            first_citation["doc_id"], second_citation["doc_id"],
            "{operation} repeated a cited section"
        );
        assert_ne!(
            first_citation["line"], second_citation["line"],
            "{operation}"
        );
    }
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

#[test]
fn rust_trait_object_and_macro_dispatch_expose_scoped_analysis_gaps() {
    let workspace = Workspace::new();
    workspace.write(
        "dispatch.rs",
        "trait Work { fn run(&self); }\n\
         struct Alpha;\n\
         struct Beta;\n\
         impl Work for Alpha { fn run(&self) {} }\n\
         impl Work for Beta { fn run(&self) {} }\n\
         macro_rules! invoke { ($value:expr) => { $value.run() }; }\n\
         fn caller(value: &dyn Work) {\n\
             value.run();\n\
             invoke!(value);\n\
         }\n",
    );
    let conn = workspace.build();
    let macro_id: String = conn
        .query_row(
            "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='dispatch.rs' AND n.name='invoke' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let implementation_ids = {
        let mut statement = conn
            .prepare(
                "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='dispatch.rs' AND n.name='run' AND n.kind='method' AND EXISTS (SELECT 1 FROM edges implementation JOIN nodes declaration ON declaration.node_hash=implementation.dst_hash WHERE implementation.src_hash=n.node_hash AND implementation.kind='implements' AND declaration.name=n.name)",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert!(implementation_ids.len() >= 2);

    let source = SourceOptions::resolve(&ResponsePolicy::default(), None, None, None, 0).unwrap();
    let mut page = options(20);
    page.detail = Detail::Full;
    let explained = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "caller",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &page,
        },
    )
    .unwrap();
    let outgoing = explained["outgoing"]["items"].as_array().unwrap();
    assert!(
        outgoing
            .iter()
            .any(|edge| edge["kind"] == "calls" && edge["dst_public_id"] == macro_id),
        "the macro invocation should remain linked to its declaration: {explained}"
    );
    assert!(
        !outgoing.iter().any(|edge| {
            edge["kind"] == "calls"
                && implementation_ids
                    .iter()
                    .any(|id| edge["dst_public_id"] == *id)
        }),
        "opaque trait-object dispatch must not become an exact implementation call: {explained}"
    );

    let gaps = explained["analysis_gaps"]["items"]
        .as_array()
        .expect("explain should expose paged analysis gaps");
    let caller_details: String = conn
        .query_row(
            "SELECT details FROM nodes WHERE id=(SELECT id FROM nodes WHERE name='caller' AND kind='function' LIMIT 1)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        explained["analysis_gaps"]["total"], 2,
        "gaps={gaps:?}; caller_details={caller_details}"
    );
    assert!(
        gaps.iter().any(|gap| {
            gap["reason"] == "trait_object_dispatch"
                && gap["path"] == "dispatch.rs"
                && gap["line"] == 8
        }),
        "trait-object uncertainty should point to the dispatch site: {gaps:?}"
    );
    assert!(
        gaps.iter().any(|gap| {
            gap["reason"] == "macro_expansion_not_analyzed"
                && gap["path"] == "dispatch.rs"
                && gap["line"] == 9
        }),
        "opaque macro expansion should point to its invocation: {gaps:?}"
    );

    let mut gap_page_options = options(1);
    let first_gap_page = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "caller",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &gap_page_options,
        },
    )
    .unwrap();
    assert_eq!(
        first_gap_page["analysis_gaps"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(first_gap_page["analysis_gaps"]["has_more"], true);
    assert!(continue_page(
        &mut gap_page_options,
        &first_gap_page["analysis_gaps"]
    ));
    let second_gap_page = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "caller",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &gap_page_options,
        },
    )
    .unwrap();
    assert_eq!(second_gap_page["analysis_gaps"]["total"], 2);
    assert_eq!(
        second_gap_page["analysis_gaps"]["items"][0]["reason"],
        "macro_expansion_not_analyzed"
    );

    let inspected = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "caller",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    assert_eq!(inspected["analysis_gaps"]["total"], 2);
    assert!(inspected.get("coverage").is_none());
}

#[test]
fn explain_pages_possible_calls_as_distinct_navigation_edges() {
    let workspace = Workspace::new();
    workspace.write(
        "branch.py",
        "class Alpha:\n    def execute(self): pass\nclass Beta:\n    def execute(self): pass\ndef run(condition):\n    if condition:\n        receiver = Alpha()\n    else:\n        receiver = Beta()\n    receiver.execute()\n",
    );
    let conn = workspace.build();
    let source = SourceOptions::resolve(&ResponsePolicy::default(), None, None, None, 0).unwrap();
    let mut page_options = options(1);

    let first = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "run",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &page_options,
        },
    )
    .unwrap();
    assert_eq!(first["outgoing"]["total"], 4);
    assert_eq!(first["outgoing"]["has_more"], true);
    let mut pages = vec![first["outgoing"]["items"][0].clone()];
    let mut current = first;
    while continue_page(&mut page_options, &current["outgoing"]) {
        current = symbols::explain_with_options(
            &conn,
            &workspace.0,
            "run",
            &symbols::ExplainOptions {
                direction: Some("outgoing"),
                show_doc: false,
                source: &source,
                coverage: CoverageOptions::default(),
                page: &page_options,
            },
        )
        .unwrap();
        pages.extend(
            current["outgoing"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
    }
    assert_eq!(pages.len(), 4);
    for edge in &pages {
        assert!(edge["src_public_id"].is_string(), "{edge}");
        assert!(edge["dst_public_id"].is_string(), "{edge}");
        assert_eq!(edge["direction"], "outbound");
        assert_eq!(edge["path"], "branch.py");
    }
    let possible = pages
        .iter()
        .filter(|edge| edge["kind"] == "possible_calls")
        .collect::<Vec<_>>();
    assert_eq!(possible.len(), 2);
    assert!(possible.iter().all(|edge| edge["confidence"] == "possible"));
    assert_ne!(possible[0]["dst_public_id"], possible[1]["dst_public_id"]);
    let all = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "run",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(100),
        },
    )
    .unwrap();
    let markdown = contextunity_forge_mcp::core::format::render_for_operation(
        &all,
        contextunity_forge_mcp::core::format::OutputFormat::Markdown,
        "code_map_explain",
        None,
        contextunity_forge_mcp::core::format::OperationKind::Navigation,
    );
    assert!(markdown.contains("[proven call]"), "{markdown}");
    assert!(markdown.contains("[heuristic / candidate]"), "{markdown}");
    assert!(
        markdown.contains("branch.py:10"),
        "callsite line is retained: {markdown}"
    );
    assert!(
        markdown.contains("possible_calls"),
        "candidate relationships remain distinct from exact calls: {markdown}"
    );
    let json = contextunity_forge_mcp::core::format::render_for_operation(
        &all,
        contextunity_forge_mcp::core::format::OutputFormat::Json,
        "code_map_explain",
        None,
        contextunity_forge_mcp::core::format::OperationKind::Navigation,
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        all
    );
    // The same persisted relationships must explain shallow impact, including
    // possible dispatch at the callsite rather than the target definition.
    for detail in [Detail::Compact, Detail::Full] {
        let mut impact_page = options(1);
        impact_page.detail = detail;
        let mut reached = Vec::new();
        loop {
            let impact = traversal::traverse_with_options(
                &conn,
                "run",
                &traversal::TraversalOptions {
                    depth: 1,
                    inbound: false,
                    mode: None,
                    edge_types: None,
                    page: &impact_page,
                },
            )
            .unwrap();
            for node in impact["nodes"]["items"].as_array().unwrap() {
                if node["is_seed"] == true {
                    assert!(node.get("via").is_none());
                    assert!(node.get("edge").is_none());
                    continue;
                }
                let edge = &node["edge"];
                let expected = pages
                    .iter()
                    .find(|edge| edge["dst_public_id"] == node["id"])
                    .unwrap();
                for field in [
                    "kind",
                    "path",
                    "line",
                    "confidence",
                    "src_public_id",
                    "dst_public_id",
                ] {
                    assert_eq!(edge[field], expected[field], "{field}: {node}");
                }
                assert_eq!(edge["direction"], "outbound");
                if edge["kind"] == "possible_calls" {
                    assert_eq!(edge["line"], 10);
                    assert_eq!(edge["confidence"], "possible");
                }
                if detail == Detail::Full {
                    assert!(edge["evidence"].is_string());
                }
                reached.push(node["id"].as_str().unwrap().to_owned());
            }
            if !continue_page(&mut impact_page, &impact["nodes"]) {
                break;
            }
        }
        assert_eq!(reached.len(), 4);
        assert_eq!(reached.iter().collect::<BTreeSet<_>>().len(), 4);
        for edge in &possible {
            let inbound = traversal::traverse_with_options(
                &conn,
                edge["dst_public_id"].as_str().unwrap(),
                &traversal::TraversalOptions {
                    depth: 1,
                    inbound: true,
                    mode: None,
                    edge_types: None,
                    page: &options(10),
                },
            )
            .unwrap();
            let caller = inbound["nodes"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == edge["src_public_id"])
                .unwrap();
            assert_eq!(caller["edge"]["kind"], "possible_calls");
            assert_eq!(caller["edge"]["direction"], "inbound");
            assert_eq!(caller["edge"]["path"], "branch.py");
            assert_eq!(caller["edge"]["line"], 10);
        }
    }
}
