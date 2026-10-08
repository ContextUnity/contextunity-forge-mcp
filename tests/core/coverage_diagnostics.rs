#![cfg(feature = "lang-python")]

use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::response::{Detail, QueryOptions, ResponsePolicy},
    db::reader,
};
use rusqlite::{params, Connection};

fn options(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

#[test]
fn diagnostics_classify_recorded_evidence_and_count_affected_files() {
    let workspace = Workspace::new();
    workspace.write("pkg/a.py", "def a(): pass\n");
    workspace.write("pkg/b.py", "def b(): pass\n");
    workspace.write("outside.py", "def outside(): pass\n");
    workspace.build();
    let conn = Connection::open(workspace.db()).unwrap();
    let cases = [
        ("pkg/a.py", 1, "dynamic", "unresolved", "computed receiver or dynamic callee; callable identity is unknown"),
        ("pkg/a.py", 2, "shadowed", "unresolved", "callee is shadowed by a parameter or local binding of unknown callable identity"),
        ("pkg/a.py", 3, "imported", "unresolved", "no indexed provider for absolute import imported; external dependency or missing source remains unverified"),
        ("pkg/a.py", 4, "call", "unresolved", "0 lexically justified candidates; implementor resolved=true, heuristic=false"),
        ("pkg/a.py", 5, "alias", "unresolved", "import pkg.a: 1 modules, 0 alias targets"),
        ("pkg/a.py", 6, "external.call", "unresolved", "call through external import json; callable target unverified"),
        ("pkg/b.py", 1, "call", "unresolved", "0 lexically justified candidates; implementor resolved=true, heuristic=false"),
        ("pkg/b.py", 2, "ambiguous", "ambiguous", "2 lexically justified candidates; implementor resolved=true, heuristic=false"),
        ("outside.py", 1, "outside", "unresolved", "0 lexically justified candidates; implementor resolved=true, heuristic=false"),
    ];
    for (path, line, expression, status, evidence) in cases {
        conn.execute(
            "INSERT OR IGNORE INTO coverage_expressions(expression) VALUES(?1)",
            [expression],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES(?1)",
            [evidence],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES((SELECT path_id FROM path_dictionary WHERE path=?1),?2,(SELECT expression_id FROM coverage_expressions WHERE expression=?3),?4,(SELECT evidence_id FROM coverage_evidence WHERE evidence=?5))",
            params![path, line, expression, status, evidence],
        )
        .unwrap();
    }
    for (path, line, message) in [
        ("pkg/a.py", 1, "syntax error"),
        ("pkg/a.py", 2, "another syntax error"),
        ("pkg/b.py", 1, "syntax error"),
        ("outside.py", 1, "outside error"),
    ] {
        conn.execute(
            "INSERT INTO errors(path,line,message) VALUES(?1,?2,?3)",
            params![path, line, message],
        )
        .unwrap();
    }

    let summary = reader::analyze_paged(&conn, "pkg", None, &options(10)).unwrap();
    assert_eq!(summary["total_unresolved"], 8);
    assert_eq!(summary["total_errors"], 3);
    assert_eq!(summary["parse_error_languages"][0]["language"], "python");
    assert_eq!(summary["parse_error_languages"][0]["errors"], 3);
    assert_eq!(summary["parse_error_languages"][0]["affected_files"], 2);
    let causes = summary["resolution_causes"].as_array().unwrap();
    let no_candidate = causes
        .iter()
        .find(|cause| cause["cause"] == "no_lexical_candidate")
        .unwrap();
    assert_eq!(no_candidate["total"], 2);
    assert_eq!(no_candidate["affected_files"], 2);
    for expected in [
        "dynamic_callee",
        "shadowed_binding",
        "missing_indexed_import",
        "alias_target_missing",
        "external_import_call",
        "ambiguous_candidates",
    ] {
        assert!(causes.iter().any(|cause| cause["cause"] == expected));
    }
    assert!(summary["cause_note"]
        .as_str()
        .unwrap()
        .contains("recorded resolver evidence"));

    let compact = reader::analyze_paged(&conn, "pkg/a.py", None, &options(10)).unwrap();
    assert_eq!(compact["resolution"]["items"][0]["cause"], "dynamic_callee");
    assert!(compact["resolution"]["items"][0].get("evidence").is_none());
    assert_eq!(compact["errors"]["items"][0]["line"], 1);
    let mut full = options(10);
    full.detail = Detail::Full;
    let full_result = reader::analyze_paged(&conn, "pkg/a.py", None, &full).unwrap();
    assert_eq!(
        full_result["resolution"]["items"][0]["cause"],
        "dynamic_callee"
    );
    assert!(full_result["resolution"]["items"][0]["evidence"]
        .as_str()
        .unwrap()
        .contains("dynamic callee"));
}
