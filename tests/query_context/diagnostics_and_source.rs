use super::*;

#[test]
fn diagnostics_summarize_directories_and_page_exact_files() {
    let workspace = Workspace::new();
    workspace.write("pkg/clean.py", "def clean(): return 1\n");
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

    let mut exact_file_options = options(7);
    let exact_file =
        reader::analyze_paged(&conn, "pkg/file_00.py", None, &exact_file_options).unwrap();
    assert_eq!(exact_file["scope"], "file");
    assert_eq!(exact_file["total_errors"], 0);
    assert_eq!(exact_file["resolution"]["total"], 45);
    assert!(exact_file["resolution"]["has_more"].as_bool().unwrap());
    assert!(exact_file.get("top_files").is_none());
    assert!(continue_page(
        &mut exact_file_options,
        &exact_file["resolution"]
    ));
    let exact_file_continuation =
        reader::analyze_paged(&conn, "pkg/file_00.py", None, &exact_file_options).unwrap();
    assert_eq!(exact_file_continuation["scope"], "file");
    assert_eq!(exact_file_continuation["resolution"]["total"], 45);
    assert_eq!(exact_file_continuation["resolution"]["offset"], 7);
    assert!(exact_file_continuation.get("top_files").is_none());

    let clean = reader::analyze_paged(&conn, "pkg/clean.py", None, &options(7)).unwrap();
    assert_eq!(clean["scope"], "file");
    assert_eq!(clean["total_errors"], 0);
    assert_eq!(clean["total_unresolved"], 0);
    assert_eq!(clean["total_external_imports"], 0);
    assert_eq!(clean["errors"]["total"], 0);
    assert_eq!(clean["resolution"]["total"], 0);
    assert_eq!(clean["external_imports"]["total"], 0);
    assert!(clean.get("top_files").is_none());

    for mode in ["diagnostics", "cycles"] {
        let error = reader::analyze_paged(&conn, mode, None, &options(7))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!(
                "analysis target '{mode}' is interpreted as a path, not an analysis mode"
            )),
            "{error}"
        );
        assert!(!error.contains("PATH_NOT_INDEXED"), "{error}");
    }
    let missing = reader::analyze_paged(&conn, "nonexistent.rs", None, &options(7))
        .unwrap_err()
        .to_string();
    assert_eq!(
        missing,
        "PATH_NOT_INDEXED: 'nonexistent.rs' was not found in the indexed inventory"
    );
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
    let cases = [116, 200, 201, 500, 527].map(|lines| {
        let name = format!("selected_{lines}");
        let body = format!(
            "def {name}():\r\n{}    return 'Привіт'\r\n",
            "    pass\r\n".repeat(lines - 2)
        );
        workspace.write(&format!("preview_{lines}.py"), &body);
        (lines, name, body)
    });
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
    let preview = SourceOptions::resolve(&policy, Some(true), None, Some(35), 0).unwrap();
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
    let next = SourceOptions::resolve(&policy, Some(true), None, Some(35), 35).unwrap();
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
    let finished = SourceOptions::resolve(&policy, Some(true), None, Some(35), 62).unwrap();
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
    let mut failures = Vec::new();
    for (lines, name, body) in cases {
        for requested in [None, Some(35), Some(500)] {
            for explain in [false, true] {
                let mut offset = 0;
                let mut query = options(30);
                let mut reconstructed = String::new();
                loop {
                    let source =
                        SourceOptions::resolve(&policy, Some(true), Some(0), requested, offset)
                            .unwrap();
                    let result = if explain {
                        symbols::explain_with_options(
                            &conn,
                            &workspace.0,
                            &name,
                            &symbols::ExplainOptions {
                                direction: None,
                                show_doc: true,
                                source: &source,
                                coverage: CoverageOptions::default(),
                                page: &query,
                            },
                        )
                    } else {
                        symbols::inspect_with_options(
                            &conn,
                            &workspace.0,
                            &name,
                            &symbols::InspectOptions {
                                show_doc: false,
                                source: &source,
                                coverage: CoverageOptions::default(),
                                page: &query,
                            },
                        )
                    }
                    .unwrap();
                    let preview = &result["source_preview"];
                    assert_eq!(preview["total_body_lines"], lines);
                    assert_eq!(preview["truncated_line"], false);
                    assert!(serde_json::to_vec(&result["source"]).unwrap().len() <= 8192);
                    if offset == 0 {
                        let expected_lines = lines.min(requested.unwrap_or(200));
                        if preview["body_lines"] != expected_lines {
                            failures.push(format!("{name}, requested={requested:?}, explain={explain}: expected {expected_lines} body lines, got {}", preview["body_lines"]));
                        }
                        let expected_more = lines > expected_lines;
                        if preview["has_more"] != expected_more {
                            failures.push(format!("{name}, requested={requested:?}, explain={explain}: expected has_more={expected_more}, got {}", preview["has_more"]));
                        }
                        if !expected_more && !preview["next_source_offset"].is_null() {
                            failures.push(format!(
                                "{name}: complete preview must not require continuation"
                            ));
                        }
                    }
                    reconstructed.push_str(result["source"].as_str().unwrap());
                    if !preview["has_more"].as_bool().unwrap() {
                        assert!(preview["next_source_offset"].is_null());
                        break;
                    }
                    let next_offset = preview["next_source_offset"].as_u64().unwrap() as usize;
                    assert!(next_offset > offset && next_offset <= lines);
                    assert_eq!(
                        next_offset,
                        offset + preview["body_lines"].as_u64().unwrap() as usize
                    );
                    offset = next_offset;
                    query.generation = Some(result["generation"].as_str().unwrap().to_owned());
                }
                assert_eq!(reconstructed, body, "{name}, explain={explain}");
            }
        }
    }
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
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn oversized_source_lines_fail_without_partial_code() {
    let workspace = Workspace::new();
    workspace.write(
        "large.py",
        &format!("def huge(): return '{}'\n", "é\\\"".repeat(12000)),
    );
    let conn = workspace.build();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(true), None, None, 0).unwrap();
    let error = symbols::inspect_with_options(
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
    .unwrap_err()
    .to_string();
    assert!(error.contains("line exceeds"), "{error}");
    assert!(error.contains("large.py"), "{error}");
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
    let empty_aspects: Vec<String> = Vec::new();
    let empty_aspect_overview = reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: Some(&empty_aspects),
            page: &options(1),
        },
    )
    .unwrap();
    assert_eq!(empty_aspect_overview, overview);
    assert!(empty_aspect_overview.get("cycles").is_none());
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

    use contextunity_forge_mcp::core::format::{self, OutputFormat};
    let mut missing = Vec::new();
    // These are recorded resolver outcomes, not inferred labels or visible-page totals.
    for (unknown, external, health) in [(0, 0, "80.0%"), (0, 12, "80.0%"), (1, 12, "79.2%")] {
        let fixture = Workspace::new();
        let linked = Workspace::new();
        fixture.write("scope.py", &"# recorded reference\n".repeat(120));
        linked.write("linked_scope.py", "def linked_target(): pass\n");
        fixture.write(
            "forge-mcp.yaml",
            &format!(
                "roots: [.]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [.]\n",
                linked.0.display()
            ),
        );
        fixture.build();
        let recorded = Connection::open(fixture.db()).unwrap();
        let populations = [
            (80, "resolved", "target", "exact indexed target"),
            (
                6,
                "unresolved",
                "callback",
                "callee is shadowed by a parameter or local binding of unknown callable identity",
            ),
            (
                4,
                "unresolved",
                "receiver.call",
                "callee is shadowed by a parameter or local binding of unknown callable identity",
            ),
            (
                3,
                "unresolved",
                "framework",
                "diagnostic_reason=framework_magic",
            ),
            (
                2,
                "unresolved",
                "external.call",
                "call through external import package; callable target unverified",
            ),
            (5, "ambiguous", "candidate", "multiple indexed candidates"),
            (
                unknown,
                "unresolved",
                "unknown",
                "unclassified resolver evidence",
            ),
            (
                external,
                "external",
                "external.import",
                "call through external import package; callable target unverified",
            ),
        ];
        let mut line = 0;
        for (count, status, expression, evidence) in populations {
            recorded
                .execute(
                    "INSERT OR IGNORE INTO coverage_expressions(expression) VALUES(?1)",
                    [expression],
                )
                .unwrap();
            recorded
                .execute(
                    "INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES(?1)",
                    [evidence],
                )
                .unwrap();
            for _ in 0..count {
                line += 1;
                recorded.execute(
                    "INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES((SELECT path_id FROM path_dictionary WHERE path='scope.py'),?1,(SELECT expression_id FROM coverage_expressions WHERE expression=?2),?3,(SELECT evidence_id FROM coverage_evidence WHERE evidence=?4))",
                    rusqlite::params![line, expression, status, evidence],
                ).unwrap();
            }
        }
        recorded
            .execute(
                "INSERT OR IGNORE INTO coverage_expressions(expression) VALUES('linked target')",
                [],
            )
            .unwrap();
        recorded
            .execute(
                "INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES('exact indexed linked target')",
                [],
            )
            .unwrap();
        recorded
            .execute(
                "INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES((SELECT path_id FROM path_dictionary WHERE path='[library]/linked_scope.py'),1,(SELECT expression_id FROM coverage_expressions WHERE expression='linked target'),'resolved',(SELECT evidence_id FROM coverage_evidence WHERE evidence='exact indexed linked target'))",
                [],
            )
            .unwrap();
        // Keep the persisted language summary consistent with the recorded population.
        recorded.execute("INSERT INTO coverage_owner_language(path_id,line,expression_id,status,evidence_id,language) SELECT path_id,line,expression_id,status,evidence_id,'python' FROM resolution_coverage", []).unwrap();
        recorded.execute("INSERT INTO coverage_language_counts(path_id,language,status,records) SELECT path_id,'python',status,count(*) FROM resolution_coverage GROUP BY path_id,status", []).unwrap();
        let diagnostics =
            reader::analyze_paged(&recorded, "scope.py", None, &options(100)).unwrap();
        let diagnostic_rows = diagnostics["resolution"]["items"].as_array().unwrap();
        for (reason, count) in [
            ("dynamic_callback", 6),
            ("unresolved_receiver_type", 4),
            ("framework_magic", 3),
            ("external_unindexed", 2),
            ("ambiguous_candidates", 5),
        ] {
            assert_eq!(
                diagnostic_rows
                    .iter()
                    .filter(|row| row["reason"] == reason && row["status"] != "external")
                    .count(),
                count,
                "fixture must use the canonical exact-file reason classifier: {reason}"
            );
        }
        assert_eq!(
            diagnostic_rows
                .iter()
                .filter(|row| row["reason"].is_null())
                .count(),
            unknown
        );
        let full_page = reader::overview_with_options(
            &recorded,
            &reader::OverviewOptions {
                aspects: None,
                page: &options(100),
            },
        )
        .unwrap();
        for limit in [1, 100] {
            let mut page_options = options(limit);
            if limit == 1 {
                page_options.offset = 1;
                page_options.generation = full_page["generation"].as_str().map(str::to_owned);
            }
            let overview = reader::overview_with_options(
                &recorded,
                &reader::OverviewOptions {
                    aspects: None,
                    page: &page_options,
                },
            )
            .unwrap();
            assert_eq!(overview["counts"]["unresolved"], 20 + unknown);
            assert_eq!(overview["counts"]["external_imports"], external);
            let json = format::render(&overview, OutputFormat::Json, "code_map_overview");
            assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), overview);
            let markdown = format::render(&overview, OutputFormat::Markdown, "code_map_overview");
            let mut expected_rows = vec![
                format!("Resolution Health: {health}"),
                "dynamic_callback): 6".to_owned(),
                "unresolved_receiver_type): 4".to_owned(),
                "framework_magic): 3".to_owned(),
                "external_unindexed, status=unresolved): 2".to_owned(),
                "ambiguous_candidates): 5".to_owned(),
                "Reason (ambiguous_candidates, status=ambiguous): 5".to_owned(),
                format!("External references: {external} (excluded from health)"),
                "external crate count: unavailable".to_owned(),
            ];
            if external > 0 {
                expected_rows.push(format!(
                    "reason=external_unindexed, status=external): {external}"
                ));
            }
            for expected in expected_rows {
                if !markdown.contains(&expected) {
                    missing.push(format!("unknown={unknown}, external={external}, limit={limit}: missing {expected:?}"));
                }
            }
            if unknown > 0
                && !(markdown.to_ascii_lowercase().contains("unknown") && markdown.contains('1'))
            {
                missing.push(
                    "unclassified recorded evidence must remain visible as unknown".to_owned(),
                );
            }
        }
        let scopes = full_page["coverage_by_workspace"]["items"]
            .as_array()
            .unwrap();
        assert_eq!(full_page["coverage_by_workspace"]["total"], 2);
        let primary = scopes
            .iter()
            .find(|scope| scope["name"] == "primary")
            .unwrap();
        let linked_scope = scopes
            .iter()
            .find(|scope| scope["name"] == "library")
            .unwrap();
        assert_eq!(
            format!(
                "{:.1}%",
                primary["resolution_health"]["percent"].as_f64().unwrap()
            ),
            health
        );
        assert_eq!(linked_scope["resolution_health"]["resolved"], 1);
        assert_eq!(linked_scope["resolution_health"]["denominator"], 1);
        let mut second_scope_page = options(1);
        second_scope_page.offset = 1;
        second_scope_page.generation = full_page["generation"].as_str().map(str::to_owned);
        let paged = reader::overview_with_options(
            &recorded,
            &reader::OverviewOptions {
                aspects: None,
                page: &second_scope_page,
            },
        )
        .unwrap();
        assert_eq!(paged["coverage_by_workspace"]["items"][0], *primary);
    }
    let empty = Workspace::new();
    empty.write("empty.py", "# no references\n");
    let empty_conn = empty.build();
    let empty_overview = reader::overview_with_options(
        &empty_conn,
        &reader::OverviewOptions {
            aspects: None,
            page: &options(1),
        },
    )
    .unwrap();
    let empty_markdown =
        format::render(&empty_overview, OutputFormat::Markdown, "code_map_overview");
    if !empty_markdown.contains("Resolution Health: unavailable") {
        missing.push("zero denominator must report unavailable, never an invented 100%".to_owned());
    }

    {
        let projected = Workspace::new();
        projected.write("source_tree/leaf.py", "def leaf(): return 1\n");
        projected.write(
            "cycles.py",
            "def recursive(): return recursive()\ndef unresolved(): return missing_target()\n",
        );
        projected.write("syntax_error.py", "def broken(:\n    pass\n");
        let conn = projected.build();
        let requested_aspects =
            ["components", "cycles", "languages", "metadata"].map(str::to_owned);
        let overview = reader::overview_with_options(
            &conn,
            &reader::OverviewOptions {
                aspects: Some(&requested_aspects),
                page: &options(100),
            },
        )
        .unwrap();

        let component_page = &overview["components"];
        let components = component_page["items"].as_array().unwrap();
        let directory = components
            .iter()
            .find(|component| {
                component["path"]
                    .as_str()
                    .is_some_and(|path| path.contains("source_tree"))
            })
            .expect("the real reader should report the source directory component");
        let directory_path = directory["path"].as_str().unwrap();

        let cycles = &overview["cycles"];
        assert!(cycles["total"].as_u64().unwrap() > 0);
        let cycle = &cycles["largest"][0];
        let cycle_path = cycle["path"].as_str().unwrap();
        let cycle_line = cycle["line"].as_u64().unwrap().to_string();

        let languages = overview["languages"]["items"].as_array().unwrap();
        let python = languages
            .iter()
            .find(|language| language["language"] == "python")
            .expect("the real reader should report the Python language row");
        for field in [
            "files",
            "resolved",
            "ambiguous",
            "unresolved",
            "external_imports",
            "parse_errors",
        ] {
            assert!(
                python[field].as_u64().is_some(),
                "the reader did not supply Python {field}: {python}"
            );
        }
        assert!(python["resolved"].as_u64().unwrap() > 0);
        assert!(python["unresolved"].as_u64().unwrap() > 0);

        let metadata = overview["metadata"].as_array().unwrap();
        let schema_version = metadata
            .iter()
            .find(|row| row["key"] == "schema_version")
            .expect("schema version should come from the reader metadata")["value"]
            .as_str()
            .unwrap();
        let workspace_root = metadata
            .iter()
            .find(|row| row["key"] == "workspace_root")
            .expect("workspace root should come from the reader metadata")["value"]
            .as_str()
            .unwrap();

        let markdown = format::render(&overview, OutputFormat::Markdown, "code_map_overview");
        assert!(
            markdown
                .to_ascii_lowercase()
                .contains("directory component"),
            "directory nodes must stay distinct from manifest-backed packages: {markdown}"
        );
        assert!(markdown.contains(directory_path), "{markdown}");
        assert!(
            markdown.to_ascii_lowercase().contains("cycle")
                && markdown.contains(cycle_path)
                && markdown.contains(&cycle_line),
            "the explicitly requested nonempty cycle summary must remain visible: {markdown}"
        );
        assert!(
            markdown.to_ascii_lowercase().contains("schema")
                && markdown.contains(schema_version)
                && markdown.contains(workspace_root),
            "the explicitly requested reader metadata must remain visible: {markdown}"
        );

        let language_section = markdown
            .split("## Languages")
            .nth(1)
            .and_then(|section| section.split("\n## ").next())
            .expect("overview Markdown should retain its language section");
        for (field, label) in [
            ("files", "files"),
            ("resolved", "resolved"),
            ("ambiguous", "ambiguous"),
            ("unresolved", "unresolved"),
            ("external_imports", "external"),
            ("parse_errors", "parse"),
        ] {
            assert!(
                language_section.to_ascii_lowercase().contains(label)
                    && language_section.contains(&python[field].as_u64().unwrap().to_string()),
                "the Python {field} value supplied by the reader must remain visible: {language_section}"
            );
        }
    }

    assert!(
        missing.is_empty(),
        "discovery overview contract: {}",
        missing.join("\n")
    );
}
