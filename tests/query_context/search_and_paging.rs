use super::*;

#[test]
fn exact_symbol_search_uses_fts_candidates_and_keeps_scope_and_kind_filters() {
    let workspace = Workspace::new();
    workspace.write("pkg/service.py", "class Server:\n    def admit(self): pass\n\ndef admission():\n    \"\"\"admit clients\"\"\"\n    pass\n");
    workspace.write("other/service.py", "def admit(): pass\n");
    let conn = workspace.build();
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read {
            table_name: "edges" | "shared_owners",
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    let exact = symbols::search_with_options(
        &conn,
        "ADMIT",
        &symbols::SearchOptions {
            kind: Some("method"),
            path: Some("pkg"),
            include_docs: false,
            exact: true,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(exact["nodes"]["total"], 1);
    assert_eq!(exact["nodes"]["items"][0]["name"], "admit");
    let qualified_name: String = conn
        .query_row(
            "SELECT qualname FROM nodes WHERE name='admit' AND path='pkg/service.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(qualified_name.ends_with("Server.admit"));
    let qualified = symbols::search_with_options(
        &conn,
        &qualified_name,
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: true,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(qualified["nodes"]["total"], 1);
    assert_eq!(
        symbols::search_with_options(
            &conn,
            "admit",
            &symbols::SearchOptions {
                kind: Some("class"),
                path: None,
                include_docs: false,
                exact: true,
                page: &options(30)
            }
        )
        .unwrap()["nodes"]["total"],
        0
    );
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let plan = reader::rows(&conn,
        "EXPLAIN QUERY PLAN SELECT n.id FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?1 AND (n.name=?2 COLLATE NOCASE OR n.qualname=?2 COLLATE NOCASE) AND (?3='' OR n.kind=?3 OR (?3='method' AND n.kind='function') OR (?3='function' AND n.kind='method')) AND (?4='' OR n.path=?4 OR (n.path>=?5 AND n.path<?6)) AND (?7=1 OR n.language!='markdown') ORDER BY n.path,n.line,n.id",
        &[&"\"admit\"", &"admit", &"method", &"pkg", &"pkg/", &"pkg0", &false], 30).unwrap();
    let plan = serde_json::to_string(&plan).unwrap();
    assert!(plan.contains("node_search"), "{plan}");
    assert!(!plan.contains("SCAN n "), "{plan}");
    let default: contextunity_forge_mcp::mcp::tools::SearchSymbols =
        serde_json::from_value(serde_json::json!({"pattern":"admit"})).unwrap();
    assert!(!default.exact);
    let enabled: contextunity_forge_mcp::mcp::tools::SearchSymbols =
        serde_json::from_value(serde_json::json!({"pattern":"admit", "exact":true})).unwrap();
    assert!(enabled.exact);
}

#[cfg(feature = "lang-rust")]
#[test]
fn prefix_search_finds_namespaced_rust_methods_using_fts_candidates() {
    let workspace = Workspace::new();
    workspace.write(
        "server.rs",
        "struct Server;\nimpl Server { fn admit(&self) {} fn reject(&self) {} }\n",
    );
    drop(workspace.build());
    let conn = Connection::open(workspace.db()).unwrap();
    conn.execute(
        "UPDATE nodes SET name='Server::admit',qualname='Server::admit' WHERE name='admit'",
        [],
    )
    .unwrap();
    let found = symbols::search_with_options(
        &conn,
        "admit*",
        &symbols::SearchOptions {
            kind: Some("method"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(found["nodes"]["total"], 1);
    assert_eq!(found["nodes"]["items"][0]["name"], "Server::admit");
    let plan = reader::rows(&conn,
        "EXPLAIN QUERY PLAN SELECT n.id FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?1 AND (n.name LIKE ?2 ESCAPE '\\' OR n.qualname LIKE ?2 ESCAPE '\\' OR n.qualname LIKE '%::' || ?2 ESCAPE '\\' OR n.qualname LIKE '%.' || ?2 ESCAPE '\\')",
        &[&"\"admit\"*", &"admit%"], 30).unwrap();
    let plan = serde_json::to_string(&plan).unwrap();
    assert!(plan.contains("VIRTUAL TABLE INDEX"), "{plan}");
    assert!(
        plan.contains("SEARCH n USING INTEGER PRIMARY KEY"),
        "{plan}"
    );
    assert!(!plan.contains("SCAN n\""), "{plan}");
}

#[test]
fn persisted_search_and_traversal_pages_have_exact_totals_without_gaps() {
    let workspace = Workspace::new();
    let mut code = String::new();
    for i in 0..73 {
        code.push_str(&format!("def leaf_{i:03}(): return {i}\n"));
    }
    code.push_str("def entry():\n");
    for i in 0..73 {
        code.push_str(&format!("    leaf_{i:03}()\n"));
    }
    workspace.write("graph.py", &code);
    let conn = workspace.build();
    let mut query = options(11);
    let mut names = Vec::new();
    loop {
        let value = symbols::search_with_options(
            &conn,
            "leaf_*",
            &symbols::SearchOptions {
                kind: Some("function"),
                path: None,
                include_docs: false,
                exact: false,
                page: &query,
            },
        )
        .unwrap();
        let page = &value["nodes"];
        assert_eq!(page["total"], 73);
        names.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["name"].as_str().unwrap().to_owned()),
        );
        if !continue_page(&mut query, page) {
            break;
        }
    }
    assert_eq!(
        names,
        (0..73).map(|i| format!("leaf_{i:03}")).collect::<Vec<_>>()
    );
    let expected = traversal::traverse_with_options(
        &conn,
        "entry",
        &traversal::TraversalOptions {
            depth: 2,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(100),
        },
    )
    .unwrap();
    let expected: Vec<_> = expected["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .collect();
    let mut query = options(9);
    let mut actual = Vec::new();
    loop {
        let value = traversal::traverse_with_options(
            &conn,
            "entry",
            &traversal::TraversalOptions {
                depth: 2,
                inbound: false,
                mode: None,
                edge_types: None,
                page: &query,
            },
        )
        .unwrap();
        let page = &value["nodes"];
        assert_eq!(page["total"], 74);
        actual.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["id"].as_str().unwrap().to_owned()),
        );
        if !continue_page(&mut query, page) {
            break;
        }
    }
    assert_eq!(actual, expected);
    assert_eq!(actual.iter().collect::<BTreeSet<_>>().len(), actual.len());
}

#[test]
fn broad_slice_is_rejected_before_recursive_pagination() {
    let workspace = Workspace::new();
    let mut source = String::new();
    for i in 0..1001 {
        source.push_str(&format!("def leaf_{i}(): pass\n"));
    }
    source.push_str("def entry():\n");
    for i in 0..1001 {
        source.push_str(&format!("    leaf_{i}()\n"));
    }
    workspace.write("graph.py", &source);
    let conn = workspace.build();
    let error = traversal::traverse_with_options(
        &conn,
        "entry",
        &traversal::TraversalOptions {
            depth: 2,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(10),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("1001 immediate graph links"), "{error}");
    assert!(error.contains("depth=1"), "{error}");
    assert!(error.contains("Reducing limit alone"), "{error}");
    let direct = traversal::traverse_with_options(
        &conn,
        "entry",
        &traversal::TraversalOptions {
            depth: 1,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(direct["nodes"]["total"], 1002);
    for (selector, direction) in [("module:graph.py", "inbound"), ("entry", "outbound")] {
        let error = symbols::tests_paged(&conn, selector, direction, &options(10))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("before its unbounded dependency walk"),
            "{error}"
        );
        assert!(error.contains("narrower module or symbol"), "{error}");
    }
}

#[test]
fn compact_queries_never_read_heavy_columns_and_full_detail_is_explicit() {
    let workspace = Workspace::new();
    workspace.write(
        "code.py",
        "# contract documentation\ndef target(): return 42\n",
    );
    workspace.write(
        "README.md",
        "# Contract\n\nArchitecture contract for target.\n",
    );
    let conn = workspace.build();
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read {
            column_name: "details" | "content" | "invariants" | "referenced_symbols",
            table_name: "nodes" | "doc_sections",
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    let compact = symbols::search_with_options(
        &conn,
        "target",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(compact["nodes"]["total"], 1);
    assert!(compact["nodes"]["items"][0].get("details").is_none());
    assert_eq!(
        symbols::search_with_options(
            &conn,
            "tar*",
            &symbols::SearchOptions {
                kind: None,
                path: None,
                include_docs: false,
                exact: false,
                page: &options(30)
            }
        )
        .unwrap()["nodes"]["total"],
        1
    );
    reader::inspect_paged(&conn, "target", false, &options(30)).unwrap();
    let docs = reader::get_doc_paged(&conn, "README.md", None, &options(30)).unwrap();
    assert!(docs["sections"]["items"][0].get("content").is_none());
    reader::search_docs_with_options(
        &conn,
        "contract",
        &reader::DocSearchOptions {
            doc_type: None,
            component: None,
            include_excerpt: false,
            page: &options(30),
        },
    )
    .unwrap();
    let mut full = options(30);
    full.detail = Detail::Full;
    assert!(symbols::search_with_options(
        &conn,
        "target",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &full
        }
    )
    .is_err());
    assert!(reader::get_doc_paged(&conn, "README.md", None, &full).is_err());
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let full_node = reader::inspect_paged(&conn, "target", false, &full).unwrap();
    assert!(full_node["node"]["details"].is_object());
}

#[test]
fn natural_symbol_queries_use_bm25_and_skip_markdown_by_default() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        "def request_cache():\n    \"\"\"Caches request state.\"\"\"\n    return None\n\ndef publish_request():\n    \"\"\"Publishes request messages to connected clients.\"\"\"\n    return None\n",
    );
    workspace.write(
        "README.md",
        "# Architecture\n\nThe architecture describes request processing.\n",
    );
    let conn = workspace.build();
    let natural = symbols::search_with_options(
        &conn,
        "publish request messages connected clients",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(natural["nodes"]["total"], 2);
    assert_eq!(natural["nodes"]["items"][0]["name"], "publish_request");

    let hidden_docs = symbols::search_with_options(
        &conn,
        "Architecture",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(hidden_docs["nodes"]["total"], 0);
    let visible_docs = symbols::search_with_options(
        &conn,
        "Architecture",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: true,
            exact: false,
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(visible_docs["nodes"]["total"], 1);
    assert_eq!(visible_docs["nodes"]["items"][0]["language"], "markdown");
}
