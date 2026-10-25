use super::*;

#[test]
fn two_phase_semantic_facts_reach_persisted_consumers_and_follow_provider_deltas() {
    // Corpus-shaped imported call -> actual template file. Ordinary declarations,
    // a valid import and a real target are prerequisites, never substitutes for
    // the exact persisted occurrence. The alias and shadow share a spelling.
    {
        let workspace = Workspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname='call-file-fixture'\ndependencies=['django']\n",
        );
        const SOURCE: &str = "from django.shortcuts import render as show_page\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n\ndef shadowed(request, show_page):\n    return show_page(request, 'catalog/card.html')\n\ndef local_render(request, template):\n    return template\n\ndef unrelated(request):\n    return local_render(request, 'catalog/card.html')\n\ndef starred(request, args):\n    return show_page(*args, 'catalog/card.html')\n\ndef comprehension(request, callbacks):\n    return [show_page(request, 'catalog/card.html') for show_page in callbacks]\n";
        const TARGET: &str = "app/templates/catalog/card.html";
        workspace.write("app/views.py", SOURCE);
        workspace.write(TARGET, "<p>Card</p>\n");
        let binding_cases = [
            ("nested-sibling", "def page(request):\n    def inner(show_page):\n        return show_page\n    return show_page(request, 'catalog/card.html')\n", Some(6)),
            ("module-rebound-before", "show_page = lambda *args: None\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("module-rebound-after", "def page(request):\n    return show_page(request, 'catalog/card.html')\nshow_page = lambda *args: None\n", None),
            ("local-rebound-before", "def page(request):\n    show_page = lambda *args: None\n    return show_page(request, 'catalog/card.html')\n", None),
            ("local-rebound-after", "def page(request):\n    value = show_page(request, 'catalog/card.html')\n    show_page = lambda *args: None\n    return value\n", None),
            ("lambda", "def page(request):\n    return (lambda show_page: show_page(request, 'catalog/card.html'))(request)\n", None),
            ("class", "class Local:\n    def page(self, request):\n        return show_page(request, 'catalog/card.html')\n", None),
            ("dynamic", "def page(request, name):\n    return show_page(request, name)\n", None),
            ("escaped", "def page(request):\n    return show_page(request, 'catalog/\\x63ard.html')\n", None),
            ("interpolated", "def page(request):\n    return show_page(request, f'catalog/{request}.html')\n", None),
            ("raw", "def page(request):\n    return show_page(request, r'catalog/card.html')\n", None),
            ("traversal", "def page(request):\n    return show_page(request, 'catalog/../card.html')\n", None),
            ("absolute", "def page(request):\n    return show_page(request, '/catalog/card.html')\n", None),
            ("keyword", "def page(request):\n    return show_page(request, template_name='catalog/card.html')\n", None),
            ("dictionary-expansion", "def page(request, options):\n    return show_page(request, 'catalog/card.html', **options)\n", None),
        ];
        for (label, body, _) in binding_cases {
            workspace.write(
                format!("cases/{label}.py"),
                &format!("from django.shortcuts import render as show_page\n\n{body}"),
            );
        }
        // Imported API identity and configured argument position are proved at
        // the complete persisted boundary for every equivalent binding form.
        let mut syntax_cases: Vec<(String, String, Option<usize>)> = [
            ("positive.py", "\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("comment-wrong-index.py", "\ndef page(request):\n    return show_page(\n        # ignored syntax trivia\n        'catalog/card.html',\n        request,\n    )\n", None),
            ("comment-valid-index.py", "\ndef page(request):\n    return show_page(\n        request,\n        # ignored syntax trivia\n        'catalog/card.html',\n    )\n", Some(4)),
            ("match-module.py", "\nmatch object():\n    case show_page:\n        pass\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("match-function.py", "\ndef page(request):\n    match request:\n        case show_page:\n            return show_page(request, 'catalog/card.html')\n", None),
            ("star-import.py", "from unknown_provider import *\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-alias.py", "type show_page = object\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-parameter.py", "\ndef page[show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("control-type-parameter.py", "\ndef page[Other](request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("control-global.py", "\ndef page(request):\n    global show_page\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("nonlocal.py", "\ndef outer():\n    show_page = lambda *args: None\n    def page(request):\n        nonlocal show_page\n        return show_page(request, 'catalog/card.html')\n    return page\n", None),
            ("class-fence.py", "\ndef outer():\n    show_page = lambda *args: None\n    class Host:\n        def page(self, request):\n            nonlocal show_page\n            return show_page(request, 'catalog/card.html')\n    return Host\n", None),
            ("type-alias-generic.py", "type show_page[T] = list[T]\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-alias-rhs-control.py", "type Other = show_page\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("type-param-bound.py", "\ndef page[show_page: object](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-variadic.py", "\ndef page[*show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-keyword.py", "\ndef page[**show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-bound-control.py", "\ndef page[Other: show_page](request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("outer-type-param.py", "\ndef outer[show_page]():\n    def page(request):\n        return show_page(request, 'catalog/card.html')\n    return page\n", None),
            ("sibling-type-param-control.py", "\ndef sibling[show_page](): pass\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("wildcard-before-explicit.py", "from unknown_provider import *\nfrom django.shortcuts import render as show_page\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("comment-before-request.py", "\ndef page(request):\n    return show_page(\n        # before the real first argument\n        request,\n        'catalog/card.html', # after the real second argument\n    )\n", Some(4)),
        ].into_iter().map(|(label, body, line)| (
            label.into(), format!("from django.shortcuts import render as show_page\n{body}"), line
        )).collect();
        for (label, pattern, unchanged_import) in [
            ("capture-bare", "show_page", false),
            ("capture-sequence", "[show_page]", false),
            ("capture-tuple", "(show_page,)", false),
            ("capture-mapping-value", "{'x': show_page}", false),
            ("capture-list-rest", "[*show_page]", false),
            ("capture-mapping-rest", "{**show_page}", false),
            ("capture-as", "_ as show_page", false),
            ("capture-union", "[show_page] | (show_page,)", false),
            ("capture-class-positional", "list([show_page])", false),
            (
                "capture-class-keyword-value",
                "object(__class__=show_page)",
                false,
            ),
            ("control-class-head", "show_page()", true),
            ("control-dotted-value", "holder.show_page", true),
            ("control-keyword-label", "object(show_page=_)", true),
            ("control-dotted-map-key", "{holder.show_page: _}", true),
            ("control-wildcard", "_", true),
            ("control-wildcard-rest", "[*_]", true),
        ] {
            for local in [false, true] {
                let body = if local {
                    format!("\ndef page(request):\n    match request:\n        case {pattern}: pass\n    return show_page(request, 'catalog/card.html')\n")
                } else {
                    format!("\nmatch object():\n    case {pattern}: pass\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n")
                };
                syntax_cases.push((
                    format!("{label}-{local}.py"),
                    format!("from django.shortcuts import render as show_page\n{body}"),
                    unchanged_import.then_some(if local { 6 } else { 7 }),
                ));
            }
        }
        workspace.write("unknown_provider.py", "def show_page(*args): return None\n");
        for (label, source, _) in &syntax_cases {
            assert_source_valid(label, "python", source, &["page"]);
            workspace.write(format!("syntax/{label}"), source);
        }
        assert_source_valid(
            "app/views.py",
            "python",
            SOURCE,
            &[
                "page",
                "shadowed",
                "local_render",
                "unrelated",
                "starred",
                "comprehension",
            ],
        );
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        let conn = workspace.open();
        assert_valid(&conn);
        let canonical_facts = persisted_facts(&conn);
        let mut call_fact_counts = std::collections::BTreeMap::new();
        for facts in &canonical_facts {
            let projected = serde_json::to_value(facts).unwrap();
            for node in projected["nodes"].as_array().unwrap() {
                let count = node["details"]
                    .get("semantic_facts")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|fact| fact["role"]["kind"] == "call_to_file")
                    .count();
                *call_fact_counts
                    .entry(node["path"].as_str().unwrap().to_owned())
                    .or_insert(0usize) += count;
            }
        }
        let navigation_dtos: i64 = conn.query_row(
            "SELECT count(*) FROM nodes WHERE json_type(details,'$.semantic_facts') IS NOT NULL",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            navigation_dtos, 0,
            "semantic DTOs belong to canonical local facts"
        );
        for (label, _, expected_line) in binding_cases {
            let path = format!("cases/{label}.py");
            let occurrences = edge_occurrence_rows(&conn, &path, "renders_template");
            let facts = call_fact_counts
                .get(path.as_str())
                .copied()
                .unwrap_or_default();
            if let Some(line) = expected_line {
                assert_eq!(
                    occurrences,
                    [(
                        node_id(&conn, &path, "page"),
                        format!("module:{TARGET}"),
                        line
                    )],
                    "{label}"
                );
                assert_eq!(facts, 1, "{label}");
            } else {
                assert!(occurrences.is_empty(), "{label}: {occurrences:?}");
                assert_eq!(
                    facts, 0,
                    "unsupported syntax/binding must not retain a call fact: {label}"
                );
            }
        }
        for (label, _, expected_line) in syntax_cases {
            let path = format!("syntax/{label}");
            let occurrences = edge_occurrence_rows(&conn, &path, "renders_template");
            let facts = call_fact_counts
                .get(path.as_str())
                .copied()
                .unwrap_or_default();
            let coverage: i64 = conn.query_row("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path=?1 AND c.status='resolved' AND c.evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='declarative file identity; unique proven target required')", [&path], |row| row.get(0)).unwrap();
            if let Some(line) = expected_line {
                assert_eq!(
                    occurrences,
                    [(
                        node_id(&conn, &path, "page"),
                        format!("module:{TARGET}"),
                        line
                    )],
                    "{label}"
                );
                assert_eq!(facts, 1, "{label}");
                assert_eq!(coverage, 1, "{label}");
            } else {
                assert!(
                    occurrences.is_empty(),
                    "unproven import or argument: {label}: {occurrences:?}"
                );
                assert_eq!(facts, 0, "{label}");
                assert_eq!(coverage, 0, "{label}");
            }
        }
        let source = node_id(&conn, "app/views.py", "page");
        let target: (String, String) = conn
            .query_row(
                "SELECT n.id,n.qualname FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='module'",
                params![TARGET],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("actual persisted HTML module target");
        assert_eq!(target.0, format!("module:{TARGET}"));
        assert_eq!(
            target.1, "app.templates.catalog.card.html",
            "actual target qualname"
        );
        let rows = edge_occurrence_rows(&conn, "app/views.py", "renders_template");
        let forbidden: Vec<_> = ["shadowed", "unrelated", "starred", "comprehension"]
            .map(|name| node_id(&conn, "app/views.py", name))
            .into_iter()
            .collect();
        assert!(
            rows.iter().all(|(owner, _, _)| !forbidden.contains(owner)),
            "shadowed/unrelated callees and unknown positional argument indexes must have no template edge: {rows:?}"
        );
        assert_eq!(
            rows,
            [(source, target.0, 4)],
            "imported alias must link the exact source occurrence to the actual module, with no guessed edges"
        );
        drop(conn);
        workspace.write(TARGET, "<p>Changed provider</p>\n");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(TARGET)).unwrap();
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(TARGET, "");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        const MOVED: &str = "app/templates/catalog/renamed.html";
        fs::rename(workspace.0.join(TARGET), workspace.0.join(MOVED)).unwrap();
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(TARGET), PathBuf::from(MOVED)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        let updated = SOURCE.replace("catalog/card.html", "catalog/renamed.html");
        workspace.write("app/views.py", &updated);
        workspace.delta("app/views.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((MOVED, 4)));
        fs::rename(
            workspace.0.join("app/views.py"),
            workspace.0.join("app/pages.py"),
        )
        .unwrap();
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from("app/views.py"), PathBuf::from("app/pages.py")],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
        fs::remove_file(workspace.0.join("app/pages.py")).unwrap();
        workspace.delta("app/pages.py");
        complete_parity(&workspace);
        assert!(
            edge_occurrence_rows(&workspace.open(), "app/pages.py", "renders_template").is_empty()
        );
        workspace.write("app/pages.py", &updated);
        workspace.delta("app/pages.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
        const REPLACE: &str = ".forge/frameworks/django.toml";
        workspace.write(REPLACE, "replace=true\n");
        workspace.delta(REPLACE);
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", None);
        fs::remove_file(workspace.0.join(REPLACE)).unwrap();
        workspace.delta(REPLACE);
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
    }
    custom_file_rules();
}
