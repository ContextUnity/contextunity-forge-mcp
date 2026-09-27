#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::response::{Detail, QueryOptions, ResponsePolicy, SourceOptions},
    db::{reader, symbols, traversal, writer},
};
use rusqlite::{
    hooks::{AuthAction, AuthContext, Authorization},
    Connection,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_query_context_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) -> Connection {
        writer::build(&self.0, &self.db(), None).unwrap();
        reader::open(&self.db(), &self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

fn continue_page(options: &mut QueryOptions, page: &Value) -> bool {
    assert_eq!(page["limit"], options.limit);
    assert_eq!(page["offset"], options.offset);
    if !page["has_more"].as_bool().unwrap() {
        assert!(page["next_offset"].is_null());
        return false;
    }
    let emitted = page["items"].as_array().unwrap().len();
    assert!(emitted > 0);
    assert_eq!(page["next_offset"], options.offset + emitted);
    options.offset += emitted;
    options.generation = Some(page["generation"].as_str().unwrap().to_owned());
    true
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
        let value = symbols::search_paged(&conn, "leaf_*", Some("function"), &query).unwrap();
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
    let expected = traversal::traverse(&conn, "entry", 2, false, 1000).unwrap();
    let expected: Vec<_> = expected["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .collect();
    let mut query = options(9);
    let mut actual = Vec::new();
    loop {
        let value = traversal::traverse_paged(&conn, "entry", 2, false, &query).unwrap();
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
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    let compact = symbols::search_paged(&conn, "target", None, &options(30)).unwrap();
    assert_eq!(compact["nodes"]["total"], 1);
    assert!(compact["nodes"]["items"][0].get("details").is_none());
    assert_eq!(
        symbols::search_paged(&conn, "tar*", None, &options(30)).unwrap()["nodes"]["total"],
        1
    );
    reader::inspect_paged(&conn, "target", false, &options(30)).unwrap();
    let docs = reader::get_doc_paged(&conn, "README.md", None, &options(30)).unwrap();
    assert!(docs["sections"]["items"][0].get("content").is_none());
    reader::search_docs_paged(&conn, "contract", None, None, &options(30)).unwrap();
    let mut full = options(30);
    full.detail = Detail::Full;
    assert!(symbols::search_paged(&conn, "target", None, &full).is_err());
    assert!(reader::get_doc_paged(&conn, "README.md", None, &full).is_err());
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let full_node = reader::inspect_paged(&conn, "target", false, &full).unwrap();
    assert!(full_node["node"]["details"].is_object());
}

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
    assert_eq!(summary["cycles"]["total"], 0);
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
    assert!(reader::analyze_paged(&conn, "DELETE FROM nodes", None, &options(8)).is_err());
    assert!(
        reader::analyze_paged(&conn, "WITH n AS (SELECT 1) DELETE FROM nodes", None, &options(8))
            .is_err()
    );
    assert!(reader::analyze_paged(&conn, "SELECT 1; SELECT 2", None, &options(8)).is_err());
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM nodes WHERE kind='function'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        73
    );
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
    assert!(symbols::inspect_paged(
        &conn,
        &workspace.0,
        "selected",
        false,
        &disabled,
        &options(30)
    )
    .unwrap()
    .get("source")
    .is_none());
    let preview = SourceOptions::resolve(&policy, Some(true), None, None, 0).unwrap();
    let first = symbols::inspect_paged(
        &conn,
        &workspace.0,
        "selected",
        false,
        &preview,
        &options(30),
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
    let last =
        symbols::inspect_paged(&conn, &workspace.0, "selected", false, &next, &query).unwrap();
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
    let empty = symbols::explain_paged(&conn, &workspace.0, "selected", None, &finished, &query).unwrap();
    assert_eq!(empty["source"], "");
    assert!(empty["source_preview"]["end_line"].is_null());
    assert!(
        symbols::inspect_paged(&conn, &workspace.0, "selected", false, &next, &options(30))
            .is_err()
    );
    workspace.write("code.py", "def selected(): return 99\n");
    assert!(
        symbols::inspect_paged(&conn, &workspace.0, "selected", false, &preview, &query).is_err()
    );
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
    let value =
        symbols::inspect_paged(&conn, &workspace.0, "huge", false, &source, &options(30)).unwrap();
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
    let first = symbols::search_paged(&conn, "*i*", Some("function"), &options(1)).unwrap();
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
    assert!(
        symbols::search_paged(&conn, "*i*", Some("function"), &query)
            .unwrap_err()
            .to_string()
            .contains("generation changed")
    );
    query.generation = None;
    assert!(symbols::search_paged(&conn, "*i*", Some("function"), &query).is_err());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn source_preview_does_not_include_neighboring_symbols_on_the_same_line() {
    let workspace = Workspace::new();
    workspace.write("same.ts", "function before() { return 'SECRET'; } function selected() { return 'selected'; } function after() { return 'AFTER'; }\n");
    let conn = workspace.build();
    let source =
        SourceOptions::resolve(&ResponsePolicy::default(), Some(true), None, None, 0).unwrap();
    let result = symbols::inspect_paged(
        &conn,
        &workspace.0,
        "selected",
        false,
        &source,
        &options(30),
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
            "SELECT id FROM nodes WHERE path='anonymous.ts' AND kind='function'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let result = symbols::inspect_paged(
        &conn,
        &workspace.0,
        &anonymous,
        false,
        &source,
        &options(30),
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
    let result = symbols::inspect_paged(
        &conn,
        &workspace.0,
        "selected",
        false,
        &source,
        &options(30),
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
    let conn = workspace.build();
    let overview = reader::overview_paged(&conn, &options(10)).unwrap();
    let langs = overview["languages"]["items"].as_array().unwrap();
    let py = langs.iter().find(|l| l["language"] == "python").unwrap();
    assert!(py["files"].as_u64().unwrap() >= 1);
    assert!(py["unresolved"].as_u64().unwrap() >= 1);
    let profiles = overview["compiled_profiles"].as_array().unwrap();
    assert!(profiles.iter().any(|p| p.as_str() == Some("python")));
}

#[test]
fn analyze_cycles_option_and_explain_direction_filtering() {
    let workspace = Workspace::new();
    workspace.write("pkg/a.py", "def a(): from pkg.b import b; b()\n");
    workspace.write("pkg/b.py", "def b(): from pkg.a import a; a()\n");
    let conn = workspace.build();

    // analyze with include_cycles = Some(false)
    let summary_no_cycles = reader::analyze_paged(&conn, "pkg", Some(false), &options(10)).unwrap();
    assert_eq!(summary_no_cycles["cycles"]["omitted"], true);

    // analyze with include_cycles = Some(true)
    let summary_with_cycles = reader::analyze_paged(&conn, "pkg", Some(true), &options(10)).unwrap();
    assert_eq!(summary_with_cycles["cycles"]["omitted"].is_null(), true);
    assert!(summary_with_cycles["cycles"]["total"].is_number());

    // explain direction filtering
    let source = SourceOptions::resolve(&ResponsePolicy::default(), Some(false), None, None, 0).unwrap();
    let both = symbols::explain_paged(&conn, &workspace.0, "a", Some("both"), &source, &options(10)).unwrap();
    assert!(both["incoming"]["total"].as_u64().unwrap() >= 1);
    assert!(both["outgoing"]["total"].as_u64().unwrap() >= 1);
    assert_eq!(both["incoming"]["omitted"].is_null(), true);
    assert_eq!(both["outgoing"]["omitted"].is_null(), true);

    let incoming_only = symbols::explain_paged(&conn, &workspace.0, "a", Some("incoming"), &source, &options(10)).unwrap();
    assert!(incoming_only["incoming"]["total"].as_u64().unwrap() >= 1);
    assert_eq!(incoming_only["incoming"]["omitted"].is_null(), true);
    assert_eq!(incoming_only["outgoing"]["omitted"], true);

    let outgoing_only = symbols::explain_paged(&conn, &workspace.0, "a", Some("outgoing"), &source, &options(10)).unwrap();
    assert_eq!(outgoing_only["incoming"]["omitted"], true);
    assert!(outgoing_only["outgoing"]["total"].as_u64().unwrap() >= 1);
    assert_eq!(outgoing_only["outgoing"]["omitted"].is_null(), true);
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
    assert!(res["scope"].as_str().unwrap().contains("unresolved reference(s)"));
}

#[test]
fn compact_coverage_includes_expression_and_evidence() {
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
    assert!(item.get("evidence").is_some());
    assert_eq!(item["status"], "unresolved");
}
