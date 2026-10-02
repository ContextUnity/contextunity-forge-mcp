use super::*;

#[test]
fn search_ranks_symbol_names_before_doc_text_and_preserves_pages() {
    let workspace = Workspace::new();
    workspace.write(
        "a.py",
        "def unrelated():\n    \"\"\"as_str helper\"\"\"\n    pass\n",
    );
    workspace.write("b.py", "def _as_str(): pass\n");
    workspace.write("z.py", "def as_str(): pass\n");
    let conn = workspace.build();
    let mut page_options = options(1);
    let mut names = Vec::new();
    let mut reasons = Vec::new();
    loop {
        let result = symbols::search_with_options(
            &conn,
            "as_str",
            &symbols::SearchOptions {
                kind: Some("function"),
                path: None,
                include_docs: false,
                exact: false,
                page: &page_options,
            },
        )
        .unwrap();
        let page = &result["nodes"];
        assert_eq!(page["total"], 3);
        let item = &page["items"][0];
        names.push(item["name"].as_str().unwrap().to_owned());
        reasons.push(
            item["match_reason"]
                .as_str()
                .unwrap_or("visible_name")
                .to_owned(),
        );
        if !continue_page(&mut page_options, page) {
            break;
        }
    }
    assert_eq!(names, ["as_str", "_as_str", "unrelated"]);
    assert_eq!(reasons, ["visible_name", "visible_name", "indexed_text"]);
    let mut full = options(10);
    full.detail = Detail::Full;
    let detailed = symbols::search_with_options(
        &conn,
        "as_str",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &full,
        },
    )
    .unwrap();
    assert_eq!(detailed["nodes"]["items"][0]["match_reason"], "exact_name");
    assert_eq!(
        detailed["nodes"]["items"][1]["match_reason"],
        "name_fragment"
    );
    let prefix = symbols::search_with_options(
        &conn,
        "as_str*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(prefix["nodes"]["total"], 1);
}

#[test]
fn search_ranking_applies_to_other_names_and_wildcard_pages() {
    let workspace = Workspace::new();
    workspace.write(
        "a.py",
        "def unrelated():\n    \"\"\"resolve_conflict helper\"\"\"\n    pass\n",
    );
    workspace.write("b.py", "def _resolve_conflict(): pass\n");
    workspace.write("z.py", "def resolve_conflict(): pass\n");
    workspace.write("build.py", "def unrelated(): pass\n");
    workspace.write("zz.py", "def build_target(): pass\n");
    let conn = workspace.build();

    let ranked = symbols::search_with_options(
        &conn,
        "resolve_conflict",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    let names = ranked["nodes"]["items"].as_array().unwrap();
    assert_eq!(ranked["nodes"]["total"], 3);
    assert_eq!(names[0]["name"], "resolve_conflict");
    assert_eq!(names[1]["name"], "_resolve_conflict");
    assert_eq!(names[2]["name"], "unrelated");

    let mut page_options = options(1);
    let mut wildcard_names = Vec::new();
    let mut reasons = Vec::new();
    loop {
        let result = symbols::search_with_options(
            &conn,
            "build*",
            &symbols::SearchOptions {
                kind: Some("function"),
                path: None,
                include_docs: false,
                exact: false,
                page: &page_options,
            },
        )
        .unwrap();
        let page = &result["nodes"];
        assert_eq!(page["total"], 2);
        wildcard_names.push(page["items"][0]["name"].as_str().unwrap().to_owned());
        reasons.push(page["items"][0]["match_reason"].as_str().map(str::to_owned));
        if !continue_page(&mut page_options, page) {
            break;
        }
    }
    assert_eq!(wildcard_names, ["build_target", "unrelated"]);
    assert_eq!(reasons, [None, Some("qualified_pattern".to_owned())]);
    let interior_wildcard = symbols::search_with_options(
        &conn,
        "*build*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(interior_wildcard["nodes"]["total"], 2);
    assert_eq!(
        interior_wildcard["nodes"]["items"][0]["name"],
        "build_target"
    );
    assert_eq!(interior_wildcard["nodes"]["items"][1]["name"], "unrelated");
}

#[test]
fn test_mapping_marks_direct_evidence_and_does_not_invent_transitive_paths() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        "def target(): pass\ndef wrapper(): target()\n",
    );
    workspace.write("tests/test_service.py", "from service import target, wrapper\ndef test_direct(): target()\ndef test_indirect(): wrapper()\n");
    let conn = workspace.build();
    let inbound = symbols::tests_paged(&conn, "target", "inbound", &options(10)).unwrap();
    let items = inbound["nodes"]["items"].as_array().unwrap();
    let direct = items
        .iter()
        .find(|item| item["name"] == "test_direct")
        .unwrap();
    assert_eq!(direct["connection"]["relation"], "direct");
    assert_eq!(direct["connection"]["edge_kind"], "calls");
    assert_eq!(direct["path"], "tests/test_service.py");
    assert!(direct["connection"].get("path").is_none());
    let indirect = items
        .iter()
        .find(|item| item["name"] == "test_indirect")
        .unwrap();
    assert_eq!(indirect["connection"]["relation"], "scope_or_transitive");
    assert!(indirect["connection"].get("path").is_none());
    let outbound = symbols::tests_paged(&conn, "test_direct", "outbound", &options(10)).unwrap();
    let target = outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "target")
        .unwrap();
    assert_eq!(target["connection"]["relation"], "direct");
    let mut full = options(10);
    full.detail = Detail::Full;
    let full_inbound = symbols::tests_paged(&conn, "target", "inbound", &full).unwrap();
    let direct = full_inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "test_direct")
        .unwrap();
    assert_eq!(direct["connection"]["path"], "tests/test_service.py");
}

#[test]
fn test_mapping_uses_lexical_fallback_when_no_test_edges_exist() {
    let workspace = Workspace::new();
    workspace.write("service.py", "def ChannelSyncEntry(): pass\n");
    workspace.write(
        "tests/test_service.py",
        "def test_ChannelSyncEntry(): pass\n",
    );
    let conn = workspace.build();
    let inbound = symbols::tests_paged(&conn, "ChannelSyncEntry", "inbound", &options(10)).unwrap();
    assert_eq!(inbound["discovery_method"], "lexical_fallback");
    assert_eq!(inbound["nodes"]["total"], 1);
    assert_eq!(
        inbound["nodes"]["items"][0]["name"],
        "test_ChannelSyncEntry"
    );
}

#[test]
fn test_mapping_places_direct_evidence_before_earlier_transitive_tests() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        "def target(): pass\ndef wrapper(): target()\n",
    );
    workspace.write(
        "tests/a_test_service.py",
        "from service import wrapper\ndef test_indirect(): wrapper()\n",
    );
    workspace.write(
        "tests/z_test_service.py",
        "from service import target\ndef test_direct(): target()\n",
    );
    let conn = workspace.build();
    let first = symbols::tests_paged(&conn, "target", "inbound", &options(1)).unwrap();
    assert_eq!(first["nodes"]["items"][0]["name"], "test_direct");
    assert_eq!(
        first["nodes"]["items"][0]["connection"]["relation"],
        "direct"
    );
    assert_eq!(first["nodes"]["total"], 2);
    let mut next = options(1);
    next.offset = 1;
    next.generation = Some(first["nodes"]["generation"].as_str().unwrap().to_owned());
    let second = symbols::tests_paged(&conn, "target", "inbound", &next).unwrap();
    assert_eq!(second["nodes"]["items"][0]["name"], "test_indirect");
    assert_eq!(
        second["nodes"]["items"][0]["connection"]["relation"],
        "scope_or_transitive"
    );
}

#[test]
fn analyze_cycles_option_and_explain_direction_filtering() {
    let workspace = Workspace::new();
    workspace.write("pkg/a.py", "def a(): from pkg.b import b; b()\n");
    workspace.write("pkg/b.py", "def b(): from pkg.a import a; a()\n");
    let conn = workspace.build();

    let summary_no_cycles = reader::analyze_paged(&conn, "pkg", None, &options(10)).unwrap();
    assert_eq!(summary_no_cycles["cycles"]["omitted"], true);

    // analyze with include_cycles = Some(true)
    let summary_with_cycles =
        reader::analyze_paged(&conn, "pkg", Some(true), &options(10)).unwrap();
    assert!(summary_with_cycles["cycles"]["omitted"].is_null());
    assert!(summary_with_cycles["cycles"]["total"].is_number());

    // explain direction filtering
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(false), None, None, 0).unwrap();
    let both = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "a",
        &symbols::ExplainOptions {
            direction: Some("both"),
            show_doc: true,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert!(both["incoming"]["total"].as_u64().unwrap() >= 1);
    assert!(both["outgoing"]["total"].as_u64().unwrap() >= 1);
    assert!(both["incoming"]["omitted"].is_null());
    assert!(both["outgoing"]["omitted"].is_null());

    let incoming_only = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "a",
        &symbols::ExplainOptions {
            direction: Some("incoming"),
            show_doc: true,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert!(incoming_only["incoming"]["total"].as_u64().unwrap() >= 1);
    assert!(incoming_only["incoming"]["omitted"].is_null());
    assert_eq!(incoming_only["outgoing"]["omitted"], true);

    let outgoing_only = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "a",
        &symbols::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: true,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(10),
        },
    )
    .unwrap();
    assert_eq!(outgoing_only["incoming"]["omitted"], true);
    assert!(outgoing_only["outgoing"]["total"].as_u64().unwrap() >= 1);
    assert!(outgoing_only["outgoing"]["omitted"].is_null());
}

#[test]
fn tests_reports_unresolved_references_in_symbol_scope() {
    let workspace = Workspace::new();
    workspace.write(
        "test_foo.py",
        "def test_one():\n    unresolved_call_target()\n",
    );
    let conn = workspace.build();
    let res = symbols::tests_paged(&conn, "test_one", "inbound", &options(10)).unwrap();
    assert!(res["unresolved_references"].as_u64().unwrap() >= 1);
    assert!(res["scope"]
        .as_str()
        .unwrap()
        .contains("unresolved reference(s)"));
}

#[test]
fn compact_coverage_keeps_scope_counts_and_full_detail_keeps_evidence() {
    let workspace = Workspace::new();
    workspace.write(
        "target.py",
        "def target_func():\n    return unresolved_dependency()\n",
    );
    let conn = workspace.build();
    let inspected = reader::inspect_paged(&conn, "target_func", false, &options(10)).unwrap();
    let coverage = &inspected["coverage"];
    assert!(coverage["total"].as_u64().unwrap() >= 1);
    let item = &coverage["items"][0];
    assert_eq!(item["expression"], "unresolved_dependency");
    assert!(item.get("evidence").is_none());
    assert!(item.get("path").is_none());
    assert_eq!(item["status"], "unresolved");
    assert!(coverage["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| { s["status"] == "unresolved" && s["total"].as_u64().unwrap() > 0 }));
    let mut full = options(10);
    full.detail = Detail::Full;
    let detailed = reader::inspect_paged(&conn, "target_func", false, &full).unwrap();
    assert_eq!(detailed["coverage"]["total"], coverage["total"]);
    assert!(detailed["coverage"]["items"][0]["evidence"].is_string());
    assert_eq!(detailed["coverage"]["items"][0]["path"], "target.py");
}

#[test]
fn compact_search_preserves_distinct_selectors_and_locations() {
    let workspace = Workspace::new();
    workspace.write(
        "a.py",
        "class First:\n    def shared(self): pass\nclass Second:\n    def shared(self): pass\n",
    );
    let conn = workspace.build();
    let compact = symbols::search_with_options(
        &conn,
        "shared",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &options(10),
        },
    )
    .unwrap();
    let mut full_options = options(10);
    full_options.detail = Detail::Full;
    let full = symbols::search_with_options(
        &conn,
        "shared",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &full_options,
        },
    )
    .unwrap();
    let items = compact["nodes"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["id"], items[1]["id"]);
    for (index, item) in items.iter().enumerate() {
        for field in ["id", "kind", "name", "path", "line", "end_line", "language"] {
            assert_eq!(item[field], full["nodes"]["items"][index][field]);
        }
        let selected =
            reader::inspect_paged(&conn, item["id"].as_str().unwrap(), false, &options(10))
                .unwrap();
        for field in ["id", "kind", "name", "path", "line", "end_line", "language"] {
            assert_eq!(selected["node"][field], item[field]);
        }
        assert!(item.get("match_reason").is_none());
    }
    assert!(serde_json::to_vec(&compact).unwrap().len() < serde_json::to_vec(&full).unwrap().len());
}

#[test]
fn compact_coverage_pages_keep_scope_totals_and_every_reference() {
    let workspace = Workspace::new();
    let mut code = String::from("def target():\n");
    for i in 0..17 {
        code.push_str(&format!("    missing_{i:02}()\n"));
    }
    workspace.write("code.py", &code);
    let conn = workspace.build();
    let mut query = options(4);
    let mut references = Vec::new();
    loop {
        let result = reader::inspect_paged(&conn, "target", false, &query).unwrap();
        let coverage = &result["coverage"];
        assert_eq!(coverage["total"], 17);
        assert_eq!(
            coverage["statuses"],
            serde_json::json!([{"status":"unresolved","total":17}])
        );
        references.extend(
            coverage["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["expression"].as_str().unwrap().to_owned()),
        );
        if !continue_page(&mut query, coverage) {
            break;
        }
    }
    assert_eq!(
        references,
        (0..17)
            .map(|i| format!("missing_{i:02}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn coverage_is_opt_in_and_snippet_preserves_source_continuation() {
    let workspace = Workspace::new();
    let mut code = String::from("def target():\n");
    for i in 0..45 {
        code.push_str(&format!("    missing_{i:02}()\n"));
    }
    workspace.write("code.py", &code);
    let conn = workspace.build();
    let policy = ResponsePolicy::default();
    let source = SourceOptions::resolve(&policy, Some(true), Some(0), None, 0).unwrap();
    let inspect = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read {
            table_name: "resolution_coverage",
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    let compact = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert!(compact.get("coverage").is_none());
    let explained = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::ExplainOptions {
            direction: None,
            show_doc: true,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert!(explained.get("coverage").is_none());
    assert!(symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "target",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions {
                include_coverage: true,
            },
            page: &options(30)
        }
    )
    .is_err());
    let snippet =
        symbols::snippet_paged(&conn, &workspace.0, "target", &source, &options(30)).unwrap();
    assert_eq!(snippet["source"], inspect["source"]);
    assert_eq!(snippet["source_preview"], inspect["source_preview"]);
    assert_eq!(snippet["node"], inspect["node"]);
    assert!(snippet.get("coverage").is_none());
    assert!(snippet.get("documents").is_none());
    let before = serde_json::to_vec(&inspect).unwrap().len();
    let after = serde_json::to_vec(&snippet).unwrap().len();
    assert!(after < before, "snippet {after} versus inspect {before}");
    let offset = snippet["source_preview"]["next_source_offset"]
        .as_u64()
        .unwrap() as usize;
    let next_source = SourceOptions::resolve(&policy, Some(true), Some(0), None, offset).unwrap();
    assert!(
        symbols::snippet_paged(&conn, &workspace.0, "target", &next_source, &options(30)).is_err()
    );
    let mut query = options(30);
    query.generation = Some(snippet["generation"].as_str().unwrap().to_owned());
    let next = symbols::snippet_paged(&conn, &workspace.0, "target", &next_source, &query).unwrap();
    assert_eq!(next["source_preview"]["has_more"], false);
    assert_eq!(
        format!(
            "{}{}",
            snippet["source"].as_str().unwrap(),
            next["source"].as_str().unwrap()
        ),
        code
    );
    query.generation = Some("stale-generation".to_owned());
    assert!(symbols::snippet_paged(&conn, &workspace.0, "target", &next_source, &query).is_err());
    workspace.write("code.py", "def target(): pass\n");
    assert!(symbols::snippet_paged(&conn, &workspace.0, "target", &source, &options(30)).is_err());
}

#[test]
fn search_docs_resolves_component_via_adapter_aliases_and_owners() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        r#"roots: [.]
eligible_roots: [extensions, packages]
doc_roots: [extensions, packages]
owners:
  extensions/commerce: commerce
  packages/core: core
aliases:
  commerce: commerce
  core: core
"#,
    );
    workspace.write(
        "extensions/commerce/docs/adr/0001-test.md",
        "# ADR-0001: Commerce Test\n\nCommerce ADR content for indexing.\n",
    );
    workspace.write(
        "packages/core/docs/adr/0001-core.md",
        "# ADR-0001: Core Architecture\n\nCore architecture documentation.\n",
    );

    let conn = workspace.build();

    let query_options = QueryOptions {
        limit: 10,
        offset: 0,
        detail: Detail::Compact,
        generation: None,
    };

    // 1. Search with exact component path
    let by_path = reader::search_docs_with_options(
        &conn,
        "Commerce",
        &reader::DocSearchOptions {
            doc_type: None,
            component: Some("extensions/commerce"),
            include_excerpt: true,
            page: &query_options,
        },
    )
    .unwrap();
    assert_eq!(by_path["sections"]["total"], 1);

    // 2. Search with logical component name resolved via adapter owners/aliases
    let by_alias = reader::search_docs_with_options(
        &conn,
        "Commerce",
        &reader::DocSearchOptions {
            doc_type: None,
            component: Some("commerce"),
            include_excerpt: true,
            page: &query_options,
        },
    )
    .unwrap();
    assert_eq!(by_alias["sections"]["total"], 1);
    assert_eq!(
        by_alias["sections"]["items"][0]["path"],
        "extensions/commerce/docs/adr/0001-test.md"
    );

    // 3. Search with non-matching component returns 0
    let by_other = reader::search_docs_with_options(
        &conn,
        "Commerce",
        &reader::DocSearchOptions {
            doc_type: None,
            component: Some("core"),
            include_excerpt: true,
            page: &query_options,
        },
    )
    .unwrap();
    assert_eq!(by_other["sections"]["total"], 0);
}
