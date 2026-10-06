use super::*;

#[test]
fn diagnostics_summarize_directories_and_page_exact_files() {
    let workspace = Workspace::new();
    for file in 0..12 {
        let mut code = String::from("def entry():\n");
        for line in 0..45 {
            code.push_str(&format!("    missing_{line}()\n"));
        }
        workspace.write(&format!("pkg/file_{file:02}.py"), &code);
    }
    let conn = workspace.build();
    let summary = reader::analyze_paged(&conn, "pkg", None, &options(7)).unwrap();
    assert_eq!(summary["scope"], "summary");
    assert_eq!(summary["total_unresolved"], 540);
    assert_eq!(summary["top_files"].as_array().unwrap().len(), 5);
    assert!(summary.get("resolution").is_none());
    assert_eq!(summary["cycles"]["omitted"], true);
    let mut query = options(7);
    let mut lines = Vec::new();
    loop {
        let value = reader::analyze_paged(&conn, "pkg/file_00.py", None, &query).unwrap();
        assert_eq!(value["scope"], "file");
        let page = &value["resolution"];
        assert_eq!(page["total"], 45);
        lines.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["line"].as_u64().unwrap()),
        );
        if !continue_page(&mut query, page) {
            break;
        }
    }
    assert_eq!(lines, (2..47).collect::<Vec<_>>());
}

#[test]
fn sql_pages_count_full_results_preserve_order_and_reject_writes() {
    let workspace = Workspace::new();
    workspace.write(
        "code.py",
        &(0..73)
            .map(|i| format!("def f{i:03}(): return {i}\n"))
            .collect::<String>(),
    );
    let conn = workspace.build();
    let sql = "WITH funcs AS (SELECT name FROM nodes WHERE kind='function') SELECT name FROM funcs ORDER BY name DESC";
    let mut query = options(8);
    let mut names = Vec::new();
    loop {
        let value = reader::analyze_paged(&conn, sql, None, &query).unwrap();
        let page = &value["rows"];
        assert_eq!(page["total"], 73);
        names.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["name"].as_str().unwrap().to_owned()),
        );
        if !continue_page(&mut query, page) {
            break;
        }
    }
    assert_eq!(
        names,
        (0..73)
            .rev()
            .map(|i| format!("f{i:03}"))
            .collect::<Vec<_>>()
    );
    let before = persisted_state(&conn);
    let attachment = workspace
        .db()
        .parent()
        .unwrap()
        .join("sql-attachment.sqlite");
    let attach_sql = format!("ATTACH DATABASE '{}' AS injected", attachment.display());
    for sql in [
        "DELETE FROM nodes",
        "DELETE FROM files",
        "WITH n AS (SELECT 1) DELETE FROM nodes",
        "UPDATE nodes SET kind='hacked'",
        "UPDATE metadata SET value='mutated' WHERE key='workspace_root'",
        "INSERT INTO metadata(key,value) VALUES ('sql-audit-sentinel','changed')",
        "DROP TABLE nodes",
        "PRAGMA user_version=32767",
        "PRAGMA journal_mode=WAL",
        "SELECT 1; SELECT 2",
        "SELECT 1; DELETE FROM nodes",
    ]
    .into_iter()
    .chain(std::iter::once(attach_sql.as_str()))
    {
        assert!(
            reader::analyze_paged(&conn, sql, None, &options(8)).is_err(),
            "accepted unsafe SQL: {sql}"
        );
        assert_eq!(
            persisted_state(&conn),
            before,
            "SQL changed database state: {sql}"
        );
        assert!(!attachment.exists(), "SQL created attached database: {sql}");
    }
}

#[test]
fn removal_safety_uses_complete_evidence_when_the_visible_page_is_empty() {
    let workspace = Workspace::new();
    workspace.write(
        "code.py",
        "def target(): return 1\ndef caller(): return target()\n",
    );
    let conn = workspace.build();
    let initial = traversal::removal_paged(&conn, "target", &options(1)).unwrap();
    assert_eq!(initial["incoming_dependencies"]["total"], 1);
    assert_eq!(initial["safe_to_remove"], false);
    let mut query = options(1);
    query.offset = 20;
    query.generation = Some(initial["generation"].as_str().unwrap().to_owned());
    let empty = traversal::removal_paged(&conn, "target", &query).unwrap();
    assert!(empty["incoming_dependencies"]["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(empty["incoming_dependencies"]["total"], 1);
    assert_eq!(empty["safe_to_remove"], false);
}

#[test]
fn source_preview_preserves_utf8_crlf_boundaries_and_line_continuation() {
    let workspace = Workspace::new();
    let body = format!(
        "def selected():\r\n{}    return 'Привіт'\r\n",
        (0..60)
            .map(|i| format!("    value_{i} = {i}\r\n"))
            .collect::<String>()
    );
    workspace.write("code.py", &format!("def previous():\r\n    return 'PREVIOUS_SECRET'\r\n\r\n# selected documentation\r\n{body}"));
    let conn = workspace.build();
    let policy = ResponsePolicy::default();
    let disabled = SourceOptions::resolve(&policy, None, None, None, 0).unwrap();
    assert!(symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &disabled,
            coverage: CoverageOptions::default(),
            page: &options(30)
        }
    )
    .unwrap()
    .get("source")
    .is_none());
    let preview = SourceOptions::resolve(&policy, Some(true), None, None, 0).unwrap();
    let first = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &preview,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    let source = first["source"].as_str().unwrap();
    assert!(source.contains("# selected documentation\r\n"));
    assert!(!source.contains("PREVIOUS_SECRET"));
    assert!(!source.contains("def previous"));
    assert_eq!(first["source_preview"]["body_lines"], 35);
    assert_eq!(first["source_preview"]["next_source_offset"], 35);
    let mut query = options(30);
    query.generation = Some(first["generation"].as_str().unwrap().to_owned());
    let next = SourceOptions::resolve(&policy, Some(true), None, None, 35).unwrap();
    let last = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &next,
            coverage: CoverageOptions::default(),
            page: &query,
        },
    )
    .unwrap();
    assert_eq!(last["source_preview"]["has_more"], false);
    assert!(last["source"]
        .as_str()
        .unwrap()
        .ends_with("return 'Привіт'\r\n"));
    let first_body = source.split_once("def selected").unwrap().1;
    assert_eq!(
        format!(
            "def selected{first_body}{}",
            last["source"].as_str().unwrap()
        ),
        body
    );
    let finished = SourceOptions::resolve(&policy, Some(true), None, None, 62).unwrap();
    let empty = symbols::explain_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::ExplainOptions {
            direction: None,
            show_doc: true,
            source: &finished,
            coverage: CoverageOptions::default(),
            page: &query,
        },
    )
    .unwrap();
    assert_eq!(empty["source"], "");
    assert!(empty["source_preview"]["end_line"].is_null());
    assert!(symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &next,
            coverage: CoverageOptions::default(),
            page: &options(30)
        }
    )
    .is_err());
    workspace.write("code.py", "def selected(): return 99\n");
    assert!(symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &preview,
            coverage: CoverageOptions::default(),
            page: &query
        }
    )
    .is_err());
}

#[test]
fn oversized_source_lines_are_bounded_with_honest_omission() {
    let workspace = Workspace::new();
    workspace.write(
        "large.py",
        &format!("def huge(): return '{}'\n", "é\\\"".repeat(12000)),
    );
    let conn = workspace.build();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(true), None, None, 0).unwrap();
    let value = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "huge",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert!(serde_json::to_vec(&value["source"]).unwrap().len() <= 8192);
    assert_eq!(value["source_preview"]["truncated_line"], true);
    assert_eq!(value["source_preview"]["omitted_leading_lines"], 0);
    assert_eq!(value["source_preview"]["has_more"], true);
    assert!(value["source_preview"]["next_source_offset"].is_null());
    assert!(value["source_preview"]["continuation_hint"]
        .as_str()
        .unwrap()
        .contains("large.py"));
}

#[test]
fn continuation_rejects_a_new_index_generation() {
    let workspace = Workspace::new();
    workspace.write("code.py", "def first(): pass\ndef second(): pass\n");
    let conn = workspace.build();
    let first = symbols::search_with_options(
        &conn,
        "*i*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options(1),
        },
    )
    .unwrap();
    let generation = first["nodes"]["generation"].as_str().unwrap().to_owned();
    drop(conn);
    workspace.write(
        "code.py",
        "def first(): pass\ndef third(): pass\ndef second(): pass\n",
    );
    writer::delta(&workspace.0, &workspace.db(), &[PathBuf::from("code.py")]).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let mut query = options(1);
    query.offset = 1;
    query.generation = Some(generation);
    assert!(symbols::search_with_options(
        &conn,
        "*i*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &query
        }
    )
    .unwrap_err()
    .to_string()
    .contains("generation changed"));
    query.generation = None;
    assert!(symbols::search_with_options(
        &conn,
        "*i*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &query
        }
    )
    .is_err());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn source_preview_does_not_include_neighboring_symbols_on_the_same_line() {
    let workspace = Workspace::new();
    workspace.write("same.ts", "function before() { return 'SECRET'; } function selected() { return 'selected'; } function after() { return 'AFTER'; }\n");
    let conn = workspace.build();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(true), None, None, 0).unwrap();
    let result = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(
        result["source"],
        "function selected() { return 'selected'; }"
    );
    drop(conn);
    workspace.write("anonymous.ts", "(function () { return 42; })();\n");
    let conn = workspace.build();
    let anonymous: String = conn
        .query_row(
            "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='anonymous.ts' AND n.kind='function'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let result = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        &anonymous,
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(result["source"], "function () { return 42; }");
}

#[cfg(feature = "lang-vue")]
#[test]
fn source_preview_resolves_vue_script_boundaries() {
    let workspace = Workspace::new();
    workspace.write("page.vue", "<template><div>{{ 'visible' }}</div></template>\n<script setup lang=\"ts\">function before() { return 'SECRET'; } function selected() { return 'visible'; }</script>\n");
    let conn = workspace.build();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(true), None, None, 0).unwrap();
    let result = symbols::inspect_with_options(
        &conn,
        &workspace.0,
        "selected",
        &symbols::InspectOptions {
            show_doc: false,
            source: &source,
            coverage: CoverageOptions::default(),
            page: &options(30),
        },
    )
    .unwrap();
    assert_eq!(
        result["source"],
        "function selected() { return 'visible'; }"
    );
}

#[test]
fn overview_reports_language_breakdown_and_compiled_profiles() {
    let workspace = Workspace::new();
    workspace.write("a.py", "def f(): return unknown_func()\n");
    #[cfg(feature = "lang-rust")]
    workspace.write("b.rs", "pub fn f() {}\n");
    let conn = workspace.build();
    let overview = reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: None,
            page: &options(1),
        },
    )
    .unwrap();
    assert!(overview["generation"].is_string());
    assert!(overview["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["key"] == "workspace_root"));
    assert!(!overview["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["key"] == "output_root" || row["key"] == "corpus_hash"));
    let langs = overview["languages"]["items"].as_array().unwrap();
    let py = langs.iter().find(|l| l["language"] == "python").unwrap();
    assert!(py["files"].as_u64().unwrap() >= 1);
    assert!(py["unresolved"].as_u64().unwrap() >= 1);
    let profiles = overview["compiled_profiles"].as_array().unwrap();
    assert!(profiles.iter().any(|p| p.as_str() == Some("python")));
    #[cfg(feature = "lang-rust")]
    {
        let mut page_options = options(1);
        assert_eq!(overview["languages"]["total"], 2);
        assert!(continue_page(&mut page_options, &overview["languages"]));
        let next = reader::overview_with_options(
            &conn,
            &reader::OverviewOptions {
                aspects: None,
                page: &page_options,
            },
        )
        .unwrap();
        assert_eq!(next["languages"]["total"], 2);
        assert_eq!(next["languages"]["items"][0]["language"], "rust");
        assert!(!next["languages"]["has_more"].as_bool().unwrap());
    }
}
