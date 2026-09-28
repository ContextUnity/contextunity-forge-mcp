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
    let error = traversal::traverse_paged(&conn, "entry", 2, false, &options(10))
        .unwrap_err()
        .to_string();
    assert!(error.contains("1001 immediate graph links"), "{error}");
    assert!(error.contains("depth=1"), "{error}");
    assert!(error.contains("Reducing limit alone"), "{error}");
    let direct = traversal::traverse_paged(&conn, "entry", 1, false, &options(10)).unwrap();
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
    assert!(reader::analyze_paged(&conn, "DELETE FROM nodes", None, &options(8)).is_err());
    assert!(reader::analyze_paged(
        &conn,
        "WITH n AS (SELECT 1) DELETE FROM nodes",
        None,
        &options(8)
    )
    .is_err());
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
    let empty =
        symbols::explain_paged(&conn, &workspace.0, "selected", None, &finished, &query).unwrap();
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
    #[cfg(feature = "lang-rust")]
    workspace.write("b.rs", "pub fn f() {}\n");
    let conn = workspace.build();
    let overview = reader::overview_paged(&conn, &options(1)).unwrap();
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
        let next = reader::overview_paged(&conn, &page_options).unwrap();
        assert_eq!(next["languages"]["total"], 2);
        assert_eq!(next["languages"]["items"][0]["language"], "rust");
        assert!(!next["languages"]["has_more"].as_bool().unwrap());
    }
}

#[test]
fn compact_inspect_and_explain_omit_repeated_empty_data_without_losing_edges() {
    let workspace = Workspace::new();
    workspace.write("graph.py", "def target(): pass\ndef caller(): target()\n");
    let conn = workspace.build();
    let compact = reader::inspect_paged(&conn, "target", false, &options(10)).unwrap();
    assert_eq!(compact["documents"], serde_json::json!({"total": 0}));
    let incoming = reader::explain_paged(&conn, "target", Some("incoming"), &options(10)).unwrap();
    let edge = &incoming["incoming"]["items"][0];
    assert_eq!(edge["kind"], "calls");
    assert!(edge["src_public_id"].is_string());
    assert!(edge.get("dst_public_id").is_none());
    assert!(incoming["incoming"]["generation"].is_string());
    let mut full = options(10);
    full.detail = Detail::Full;
    let full_edge = reader::explain_paged(&conn, "target", Some("incoming"), &full).unwrap();
    assert_eq!(
        full_edge["incoming"]["items"][0]["dst_public_id"],
        full_edge["node"]["id"]
    );
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
    let basic = reader::search_docs_paged(&conn, "needle", None, None, &options(10)).unwrap();
    assert!(basic["sections"]["items"][0].get("excerpt").is_none());
    let with_excerpt =
        reader::search_docs_paged_with_excerpt(&conn, "needle", None, None, true, &options(10))
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
        let result =
            reader::search_docs_paged_with_excerpt(&conn, query, None, None, true, &options(10))
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
    let all = symbols::search_paged(&conn, "lookup", Some("function"), &options(10)).unwrap();
    assert_eq!(all["nodes"]["total"], 3);
    let directory = symbols::search_paged_in_path(
        &conn,
        "lookup",
        Some("function"),
        Some("./pkg"),
        &options(1),
    )
    .unwrap();
    assert_eq!(directory["nodes"]["total"], 2);
    assert_eq!(directory["nodes"]["items"][0]["path"], "pkg/a.py");
    assert!(directory["nodes"]["has_more"].as_bool().unwrap());
    let file = symbols::search_paged_in_path(
        &conn,
        "lookup",
        Some("function"),
        Some("file:pkg/sub/b.py"),
        &options(10),
    )
    .unwrap();
    assert_eq!(file["nodes"]["total"], 1);
    assert_eq!(file["nodes"]["items"][0]["path"], "pkg/sub/b.py");
    for invalid in ["", "../other.py", "/tmp/other.py", "pkg//sub", "pkg\\sub"] {
        assert!(
            symbols::search_paged_in_path(&conn, "lookup", None, Some(invalid), &options(10))
                .is_err(),
            "{invalid}"
        );
    }
}

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
        let result =
            symbols::search_paged(&conn, "as_str", Some("function"), &page_options).unwrap();
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
    let detailed = symbols::search_paged(&conn, "as_str", Some("function"), &full).unwrap();
    assert_eq!(detailed["nodes"]["items"][0]["match_reason"], "exact_name");
    assert_eq!(
        detailed["nodes"]["items"][1]["match_reason"],
        "name_fragment"
    );
    let prefix = symbols::search_paged(&conn, "as_str*", Some("function"), &options(10)).unwrap();
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

    let ranked =
        symbols::search_paged(&conn, "resolve_conflict", Some("function"), &options(10)).unwrap();
    let names = ranked["nodes"]["items"].as_array().unwrap();
    assert_eq!(ranked["nodes"]["total"], 3);
    assert_eq!(names[0]["name"], "resolve_conflict");
    assert_eq!(names[1]["name"], "_resolve_conflict");
    assert_eq!(names[2]["name"], "unrelated");

    let mut page_options = options(1);
    let mut wildcard_names = Vec::new();
    let mut reasons = Vec::new();
    loop {
        let result =
            symbols::search_paged(&conn, "build*", Some("function"), &page_options).unwrap();
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
    let interior_wildcard =
        symbols::search_paged(&conn, "*build*", Some("function"), &options(10)).unwrap();
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
    let both = symbols::explain_paged(
        &conn,
        &workspace.0,
        "a",
        Some("both"),
        &source,
        &options(10),
    )
    .unwrap();
    assert!(both["incoming"]["total"].as_u64().unwrap() >= 1);
    assert!(both["outgoing"]["total"].as_u64().unwrap() >= 1);
    assert!(both["incoming"]["omitted"].is_null());
    assert!(both["outgoing"]["omitted"].is_null());

    let incoming_only = symbols::explain_paged(
        &conn,
        &workspace.0,
        "a",
        Some("incoming"),
        &source,
        &options(10),
    )
    .unwrap();
    assert!(incoming_only["incoming"]["total"].as_u64().unwrap() >= 1);
    assert!(incoming_only["incoming"]["omitted"].is_null());
    assert_eq!(incoming_only["outgoing"]["omitted"], true);

    let outgoing_only = symbols::explain_paged(
        &conn,
        &workspace.0,
        "a",
        Some("outgoing"),
        &source,
        &options(10),
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
    let compact = symbols::search_paged(&conn, "shared", None, &options(10)).unwrap();
    let mut full_options = options(10);
    full_options.detail = Detail::Full;
    let full = symbols::search_paged(&conn, "shared", None, &full_options).unwrap();
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
fn snippet_avoids_coverage_reads_and_preserves_source_continuation() {
    let workspace = Workspace::new();
    let mut code = String::from("def target():\n");
    for i in 0..45 {
        code.push_str(&format!("    missing_{i:02}()\n"));
    }
    workspace.write("code.py", &code);
    let conn = workspace.build();
    let policy = ResponsePolicy::default();
    let source = SourceOptions::resolve(&policy, Some(true), Some(0), None, 0).unwrap();
    let inspect =
        symbols::inspect_paged(&conn, &workspace.0, "target", false, &source, &options(30))
            .unwrap();
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read {
            table_name: "resolution_coverage",
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    assert!(
        symbols::inspect_paged(&conn, &workspace.0, "target", false, &source, &options(30))
            .is_err()
    );
    let snippet =
        symbols::snippet_paged(&conn, &workspace.0, "target", &source, &options(30)).unwrap();
    assert_eq!(snippet["source"], inspect["source"]);
    assert_eq!(snippet["source_preview"], inspect["source_preview"]);
    assert_eq!(snippet["node"], inspect["node"]);
    assert!(snippet.get("coverage").is_none());
    assert!(snippet.get("documents").is_none());
    let before = serde_json::to_vec(&inspect).unwrap().len();
    let after = serde_json::to_vec(&snippet).unwrap().len();
    assert!(
        after * 2 < before,
        "snippet {after} versus inspect {before}"
    );
    eprintln!("snippet fixture: inspect={before} bytes, snippet={after} bytes");
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
