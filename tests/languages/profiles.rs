use super::support::Workspace;
use contextunity_forge_mcp::db::{reader, writer};
use contextunity_forge_mcp::engine::ast;
use std::{fs, path::PathBuf};

fn persisted_graph(w: &Workspace) -> Vec<Vec<String>> {
    let conn = reader::open(&w.db(), &w.0).unwrap();
    [
        "SELECT id||'|'||qualname||'|'||details FROM nodes ORDER BY id",
        "SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)||'|'||line||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.confidence_id) FROM edge_occurrences ORDER BY (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),kind,line",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
    ].iter().map(|sql| {
        conn.prepare(sql).unwrap().query_map([], |r|r.get(0)).unwrap().collect::<Result<Vec<String>,_>>().unwrap()
    }).collect()
}

#[cfg(feature = "lang-go")]
#[test]
fn go_sibling_functions_in_same_package_link() {
    let w = Workspace::new();
    w.write(
        "pkg/helper.go",
        r#"package mypkg

func Helper() int {
    return 42
}
"#,
    );
    w.write(
        "pkg/main.go",
        r#"package mypkg

func Main() int {
    return Helper()
}
"#,
    );
    let conn = w.build();

    let nodes: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, name, qualname FROM nodes")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("NODES: {:?}", nodes);
    let cov: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id), status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("COVERAGE: {:?}", cov);

    let calls: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='calls'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(
        calls.len(),
        1,
        "expected 1 call edge between sibling files in same package: {:?}",
        calls
    );
    assert!(calls[0].0.contains("Main"));
    assert!(calls[0].1.contains("Helper"));

    let resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE status='resolved' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Helper'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, 1, "expected Helper to be resolved");
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_macros_and_cargo_workspace() {
    let w = Workspace::new();
    w.write(
        "crates/core/src/macros.rs",
        r#"#[macro_export]
macro_rules! log_info {
    ($msg:expr) => {
        println!("{}", $msg)
    };
}

"#,
    );
    w.write(
        "crates/core/src/lib.rs",
        r#"pub mod macros;

use crate::macros::log_info;

pub fn execute() {
    log_info!("running");
    println!("standard macro");
}
"#,
    );

    let conn = w.build();

    // Verify macro node is indexed
    let macro_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='macro' AND name='log_info'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        macro_count, 1,
        "expected log_info macro to be extracted as kind='macro'"
    );

    // Verify standard-library println retains external provenance.
    let println_external: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='println' AND status='external'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        println_external, 1,
        "expected println macro to retain standard-library provenance"
    );

    // Verify crate:: import resolved to crates.core.src prefix
    let import_resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id) LIKE '%log_info%' AND status='resolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        import_resolved >= 1,
        "expected crate:: import of log_info to be resolved"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_manifest_types_preserve_receiver_origin_for_aliases_and_qualified_names() {
    let w = Workspace::new();
    w.write("Cargo.toml", "[package]\nname = \"receiver-provenance\"\nversion = \"0.1.0\"\n[dependencies]\nstorage-api = \"1\"\n");
    w.write("src/lib.rs", "use storage_api::Connection as DbConnection;\nfn imported(conn: &DbConnection) { conn.query(); }\nfn qualified(conn: &storage_api::Connection) { conn.query(); }\nfn generic(conn: &storage_api::Connection<u8>) { conn.query(); }\nfn associated(conn: &storage_api::Connection<u8>::Item<u8>) { conn.query(); }\nstruct Local;\nfn local(value: &Local) { value.query(); }\nfn undeclared(value: &missing_api::Connection) { value.query(); }\n");
    let conn = w.build();
    let mut statement = conn.prepare("SELECT c.line, e.expression, c.status, v.evidence FROM resolution_coverage c JOIN coverage_expressions e ON e.expression_id = c.expression_id JOIN coverage_evidence v ON v.evidence_id = c.evidence_id WHERE e.expression IN ('conn.query', 'value.query') ORDER BY c.line").unwrap();
    let rows: Vec<(i64, String, String, String)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for line in [2, 3, 4] {
        assert!(
            rows.iter()
                .any(|(at, expression, status, evidence)| *at == line
                    && expression == "conn.query"
                    && status == "external"
                    && evidence.contains("storage_api")),
            "{rows:#?}"
        );
    }
    assert!(
        rows.iter().any(|(at, expression, status, _)| *at == 5
            && expression == "conn.query"
            && status == "unresolved"),
        "{rows:#?}"
    );
    for line in [7, 8] {
        assert!(
            rows.iter().any(|(at, expression, status, _)| *at == line
                && expression == "value.query"
                && status == "unresolved"),
            "{rows:#?}"
        );
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn persisted_imports_require_complete_language_and_workspace_identity() {
    let owner = Workspace::new();
    let linked = Workspace::new();
    owner.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: provider\n    path: '{}'\n    roots: [.]\n",
            linked.0.display()
        ),
    );
    owner.write("main.py", "from settings import value\nfrom path import Path\nimport time\nfrom typing import Any\nfrom commerce.snapshot import revert\ndef use():\n    value()\n    Path()\n    time.time()\n    Any()\n    revert()\n");
    linked.write("package/settings.py", "def value(): pass\n");
    linked.write("package/path.py", "class Path: pass\n");
    linked.write("package/time.py", "def time(): pass\n");
    linked.write("package/typing.py", "class Any: pass\n");
    linked.write("commerce/snapshot.py", "def revert(): pass\n");
    owner.build();
    let conn = reader::open(&owner.db(), &owner.0).unwrap();
    let mut st = conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)='main.py' AND kind IN ('imports','calls') ORDER BY (SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)").unwrap();
    let targets: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        targets.len(),
        3,
        "the qualified module import, symbol import, and call must resolve: {targets:?}"
    );
    assert!(targets.iter().all(|t| t.contains("commerce/snapshot.py")));
    assert_eq!(
        targets
            .iter()
            .filter(|target| target.starts_with("module:"))
            .count(),
        1
    );
    assert_eq!(
        targets
            .iter()
            .filter(|target| target.ends_with(":revert"))
            .count(),
        2
    );
    let unresolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='main.py' AND status='unresolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        unresolved, 0,
        "unmatched absolute imports and calls are retained as unverified external dependencies"
    );
    let external: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='main.py' AND status='external'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        external, 8,
        "four unmatched absolute imports and their calls are classified as external"
    );
    let unverified_external: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='main.py' AND status='external' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id) IN ('value','Path') AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) LIKE '%no indexed provider%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unverified_external, 2);
}

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
        let f = ast::extract(path, language, "").unwrap();
        assert_eq!(f.nodes[0].qualname, expected, "{path}");
    }
}

#[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
#[test]
fn import_owner_family_relative_and_ambiguous_provider_matrix() {
    let owner = Workspace::new();
    let a = Workspace::new();
    let b = Workspace::new();
    owner.write("forge-mcp.yaml", &format!("roots: [.]\nlinked_workspaces:\n  - name: a\n    path: '{}'\n    roots: [.]\n  - name: b\n    path: '{}'\n    roots: [.]\n",a.0.display(),b.0.display()));
    owner.write("settings.py", "def value(): pass\n");
    owner.write("shared/service.py", "def local(): pass\n");
    owner.write("foreign/only.ts", "export function target() {}\n");
    owner.write("main.py","from settings import value\nfrom shared.service import local\nfrom foreign.only import target\nfrom dup.provider import chosen\ndef use():\n    value()\n    local()\n    target()\n    chosen()\n");
    a.write("settings.py", "def value(): pass\n");
    a.write("shared/service.py", "def local(): pass\n");
    a.write("pkg/helpers.py", "def helper(): pass\n");
    a.write("pkg/use.py","from .helpers import helper\nfrom ...settings import value\ndef use():\n    helper()\n    value()\n");
    a.write("dup/provider.py", "def chosen(): pass\n");
    b.write("dup/provider.py", "def other(): pass\n");
    owner.build();
    let conn = reader::open(&owner.db(), &owner.0).unwrap();
    let mut st = conn
        .prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id), (SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash) FROM edge_occurrences WHERE kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)")
        .unwrap();
    let calls: Vec<(String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(calls.contains(&("main.py".into(), "py:settings.py:1:value".into())));
    assert!(calls.contains(&("main.py".into(), "py:shared/service.py:1:local".into())));
    assert!(calls.contains(&(
        "[a]/pkg/use.py".into(),
        "py:[a]/pkg/helpers.py:1:helper".into()
    )));
    let ambiguous:i64=conn.query_row("SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='main.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='chosen' AND status='ambiguous'",[],|r|r.get(0)).unwrap();
    assert_eq!(ambiguous, 1);
}

#[cfg(feature = "lang-python")]
#[test]
fn import_provider_changes_delta_matches_cold_build() {
    let owner = Workspace::new();
    let linked = Workspace::new();
    owner.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: provider\n    path: '{}'\n    roots: [.]\n",
            linked.0.display()
        ),
    );
    owner.write(
        "main.py",
        "from pkg.service import run\ndef use(): return run()\n",
    );
    linked.write("pkg/service.py", "def run(): pass\n");
    owner.build();
    for step in 0..5 {
        let changed = match step {
            0 => {
                linked.write("pkg/service.py", "\ndef run(): pass\n");
                vec!["[provider]/pkg/service.py"]
            }
            1 => {
                fs::rename(
                    linked.0.join("pkg/service.py"),
                    linked.0.join("pkg/renamed.py"),
                )
                .unwrap();
                vec!["[provider]/pkg/service.py", "[provider]/pkg/renamed.py"]
            }
            2 => {
                linked.write("pkg/service.py", "def run(): pass\n");
                vec!["[provider]/pkg/service.py"]
            }
            3 => {
                linked.write("pkg/service.pyi", "def unrelated(): pass\n");
                vec!["[provider]/pkg/service.pyi"]
            }
            _ => {
                fs::remove_file(linked.0.join("pkg/service.pyi")).unwrap();
                vec!["[provider]/pkg/service.pyi"]
            }
        };
        writer::delta(
            &owner.0,
            &owner.db(),
            &changed.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )
        .unwrap();
        let delta = persisted_graph(&owner);
        owner.build();
        let cold = persisted_graph(&owner);
        assert_eq!(delta, cold, "provider change step {step}");
    }
}

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
            profile.create_parser(&path).unwrap();
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
#[test]
fn popular_languages_persist_calls_relations_diagnostics_and_delta() {
    let fixtures: &[(&str, &str, &str, i64)] = &[
        #[cfg(feature = "lang-java")]
        (
            "sample.java",
            "java",
            r#"import missing.Library;
interface Face {}
class Base {}
class Worker extends Base implements Face {
 int helper() { return 1; }
 // caller documentation
 int caller() { return helper(); }
 void unknown(Thing client) { client.run(); }
 void shadow(Callback helper) { helper(); }
}

"#,
            2,
        ),
        #[cfg(feature = "lang-csharp")]
        (
            "sample.cs",
            "csharp",
            r#"using Missing.Library;
interface Face {}
class Base {}
class Worker : Base, Face {
 int helper() { return 1; }
 // caller documentation
 int caller() { return helper(); }
 void unknown(dynamic client) { client.run(); }
 void shadow(System.Func<int> helper) { helper(); }
}
"#,
            2,
        ),
        #[cfg(feature = "lang-kotlin")]
        (
            "sample.kt",
            "kotlin",
            r#"import missing.Library
interface Face
open class Base
class Worker : Base(), Face {
 fun helper(): Int { return 1 }
 // caller documentation
 fun caller(): Int { return helper() }
 fun unknown(client: Thing) { client.run() }
 fun shadow(helper: () -> Int) { helper() }
}
"#,
            2,
        ),
        #[cfg(feature = "lang-php")]
        (
            "sample.php",
            "php",
            r#"<?php
use Missing\Library;
interface Face {}
class Base {}
class Worker extends Base implements Face {}
function helper() { return 1; }
// caller documentation
function caller() { return helper(); }
function unknown($client) { $client->run(); }
function shadow($helper) { $helper(); }
"#,
            2,
        ),
        #[cfg(feature = "lang-ruby")]
        (
            "sample.rb",
            "ruby",
            r#"require 'missing/library'
class Base
end
class Worker < Base
 def helper()
  1
 end
 # caller documentation
 def caller()
  helper()
 end
 def unknown(client)
  client.run()
 end
 def shadow(helper)
  helper.call()
 end
end
"#,
            1,
        ),
        #[cfg(feature = "lang-c")]
        (
            "sample.c",
            "c",
            r#"#include <missing.h>
int helper(void) { return 1; }
// caller documentation
int caller(void) { return helper(); }
void unknown(struct Thing *client) { client->run(); }
int shadow(int (*helper)(void)) { return helper(); }
"#,
            0,
        ),
        #[cfg(feature = "lang-cpp")]
        (
            "sample.cpp",
            "cpp",
            r#"#include <missing.hpp>
class Base {};
class Worker : public Base {
 int helper() { return 1; }
 // caller documentation
 int caller() { return helper(); }
 void unknown(Thing client) { client.run(); }
 int shadow(int (*helper)()) { return helper(); }
};
"#,
            1,
        ),
    ];
    for (path, language, source, relations) in fixtures.iter().copied() {
        let w = Workspace::new();
        w.write(path, source);
        w.build();
        let conn = reader::open(&w.db(), &w.0).unwrap();
        let errors: i64 = conn
            .query_row("SELECT count(*) FROM errors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            errors,
            0,
            "{language}: {:?}",
            ast::extract(path, language, source).unwrap().errors
        );
        let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
            &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
            Some(100),
            0,
            None,
            None,
        )
        .unwrap();
        let caller = reader::inspect_paged(&conn, "caller", true, &page).unwrap();
        assert_eq!(caller["node"]["language"], language);
        let line = caller["node"]["line"].as_u64().unwrap() as usize;
        assert!(
            source.lines().nth(line - 1).unwrap().contains("caller"),
            "{language}: {caller}"
        );
        assert!(
            caller["node"]["details"]["doc"]
                .as_str()
                .unwrap_or("")
                .contains("caller documentation"),
            "{language}: {caller}"
        );
        let mut st=conn.prepare("SELECT s.name,d.name FROM edge_occurrences e JOIN nodes s ON s.id=(SELECT id FROM nodes WHERE node_hash=e.src_hash) JOIN nodes d ON d.id=(SELECT id FROM nodes WHERE node_hash=e.dst_hash) WHERE e.kind='calls' ORDER BY s.name,d.name").unwrap();
        let calls: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut expected = vec![("caller".into(), "helper".into())];
        if language == "java" {
            expected.push(("shadow".into(), "helper".into()));
        }
        assert_eq!(calls, expected, "{language}");
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM edge_occurrences WHERE kind IN('inherits','implements')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, relations, "{language}");
        let unknown: i64 = conn
            .query_row(
                "SELECT count(*) FROM resolution_coverage WHERE status='unresolved'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            unknown >= if language == "java" { 2 } else { 3 },
            "{language}: {unknown}"
        );
        drop(st);
        drop(conn);
        let edited = source.replace("helper", "renamed");
        let edited = if language == "php" {
            edited.replacen("<?php", "<?php\n", 1)
        } else {
            format!("\n{edited}")
        };
        w.write(path, &edited);
        writer::delta(&w.0, &w.db(), &[PathBuf::from(path)]).unwrap();
        let delta = persisted_graph(&w);
        w.build();
        assert_eq!(delta, persisted_graph(&w), "{language}");
        let malformed = if language == "php" {
            "<?php function broken( {"
        } else {
            "\0"
        };
        w.write(path, malformed);
        w.build();
        let conn = reader::open(&w.db(), &w.0).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM errors", [], |r| r.get(0))
            .unwrap();
        assert!(count > 0, "{language} malformed syntax has diagnostics");
    }
}
