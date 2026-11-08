use crate::languages::harness::ExtractorHarness;
use crate::languages::PersistedWorkspaceHarness;
use contextunity_forge_mcp::engine::ast;
use std::fs;

#[cfg(feature = "lang-typescript")]
mod legacy_hooks;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]
mod baseline;
#[cfg(all(feature = "lang-python", feature = "lang-rust"))]
mod cache;

fn persisted_graph(w: &PersistedWorkspaceHarness) -> Vec<Vec<String>> {
    let conn = w.open();
    [
        "SELECT id||'|'||qualname||'|'||details FROM nodes ORDER BY id",
        "SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)||'|'||line||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.confidence_id) FROM edge_occurrences ORDER BY (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),kind,line",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
    ].iter().map(|sql| {
        conn.prepare(sql).unwrap().query_map([], |r|r.get(0)).unwrap().collect::<Result<Vec<String>,_>>().unwrap()
    }).collect()
}

mod workspaces;

#[cfg(feature = "lang-rust")]
mod forwarding;

#[cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]
#[test]
fn module_conventions_are_language_specific() {
    for (path, language, expected) in [
        #[cfg(feature = "lang-python")]
        ("pkg/index.py", "python", "pkg.index"),
        #[cfg(feature = "lang-python")]
        ("pkg/mod.py", "python", "pkg.mod"),
        #[cfg(feature = "lang-python")]
        ("pkg/__init__.py", "python", "pkg"),
        #[cfg(feature = "lang-typescript")]
        ("pkg/index.ts", "typescript", "pkg"),
        #[cfg(feature = "lang-typescript")]
        ("pkg/mod.ts", "typescript", "pkg.mod"),
        #[cfg(feature = "lang-rust")]
        ("pkg/mod.rs", "rust", "pkg"),
        #[cfg(feature = "lang-rust")]
        ("pkg/index.rs", "rust", "pkg.index"),
    ] {
        let f = ExtractorHarness::for_lang(language)
            .at_path(path)
            .extract("");
        assert_eq!(f.nodes[0].qualname, expected, "{path}");
    }
}

mod imports;

#[test]
fn automatic_registry_has_unique_languages_and_extensions() {
    use contextunity_forge_mcp::engine::languages;
    let profiles: Vec<_> = languages::profiles().collect();
    languages::validate_profiles(profiles.iter().copied()).unwrap();
    if let Some(first) = profiles.first() {
        assert!(languages::validate_profiles([*first, *first]).is_err());
    }
    for profile in profiles {
        for extension in profile.extensions() {
            let path = format!("sample.{extension}");
            assert_eq!(
                languages::for_path(std::path::Path::new(&path))
                    .unwrap()
                    .id(),
                profile.id()
            );
            if profile.has_grammar() {
                profile.create_parser(&path).unwrap();
            } else {
                assert!(profile.create_parser(&path).is_err());
            }
        }
    }
}

#[cfg(any(
    feature = "lang-java",
    feature = "lang-csharp",
    feature = "lang-kotlin",
    feature = "lang-php",
    feature = "lang-ruby",
    feature = "lang-c",
    feature = "lang-cpp"
))]
mod popular;

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}
