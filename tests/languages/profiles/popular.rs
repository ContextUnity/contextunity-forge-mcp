use super::*;
use contextunity_forge_mcp::db::reader;

const FIXTURES: &[(&str, &str, &str, i64)] = &[
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

type CoverageRow = (i64, &'static str, &'static str, &'static str);
fn expected_coverage(language: &str) -> Vec<CoverageRow> {
    let resolved = "1 lexically justified candidates; implementor resolved=true, heuristic=false";
    let shadowed =
        "callee is shadowed by a parameter or local binding of unknown callable identity";
    let dynamic = "computed receiver or dynamic callee; callable identity is unknown";
    match language {
        "java" => vec![
            (
                1,
                "missing.Library",
                "external",
                "external dependency; no indexed provider for missing.Library",
            ),
            (4, "Base", "resolved", resolved),
            (4, "Face", "resolved", resolved),
            (7, "helper", "resolved", resolved),
            (8, "client.run", "unresolved", shadowed),
            (9, "helper", "resolved", resolved),
        ],
        "csharp" => vec![
            (
                1,
                "Missing.Library",
                "unresolved",
                "import Missing.Library: 0 modules, 0 alias targets",
            ),
            (4, "Base", "resolved", resolved),
            (4, "Face", "resolved", resolved),
            (7, "helper", "resolved", resolved),
            (8, "client.run", "unresolved", shadowed),
            (9, "helper", "unresolved", shadowed),
        ],
        "kotlin" => vec![
            (
                1,
                "missing.Library",
                "external",
                "external dependency; no indexed provider for missing.Library",
            ),
            (4, "Base", "resolved", resolved),
            (4, "Base()", "unresolved", dynamic),
            (4, "Face", "resolved", resolved),
            (7, "helper", "resolved", resolved),
            (8, "client.run", "unresolved", shadowed),
            (9, "helper", "unresolved", shadowed),
        ],
        "php" => vec![
            (
                2,
                "Missing\\Library",
                "unresolved",
                "import Missing\\Library: 0 modules, 0 alias targets",
            ),
            (5, "Base", "resolved", resolved),
            (5, "Face", "resolved", resolved),
            (8, "helper", "resolved", resolved),
            (9, "$client.run", "unresolved", dynamic),
            (10, "$helper", "unresolved", dynamic),
        ],
        "ruby" => vec![
            (
                1,
                "'missing/library'",
                "unresolved",
                "import 'missing/library': 0 modules, 0 alias targets",
            ),
            (4, "Base", "resolved", resolved),
            (10, "helper", "resolved", resolved),
            (13, "client", "unresolved", dynamic),
            (13, "client.run", "unresolved", shadowed),
            (16, "helper", "unresolved", dynamic),
            (16, "helper.call", "unresolved", shadowed),
        ],
        "c" => vec![
            (
                1,
                "<missing.h>",
                "unresolved",
                "import <missing.h>: 0 modules, 0 alias targets",
            ),
            (4, "helper", "resolved", resolved),
            (5, "client->run", "unresolved", dynamic),
            (6, "helper", "unresolved", shadowed),
        ],
        "cpp" => vec![
            (
                1,
                "<missing.hpp>",
                "unresolved",
                "import <missing.hpp>: 0 modules, 0 alias targets",
            ),
            (3, "Base", "resolved", resolved),
            (6, "helper", "resolved", resolved),
            (7, "client.run", "unresolved", shadowed),
            (8, "helper", "unresolved", shadowed),
        ],
        _ => unreachable!("fixture language is unsupported"),
    }
}

#[test]
fn popular_languages_preserve_calls_coverage_deltas_and_diagnostics() {
    for &(path, language, source, relations) in FIXTURES {
        let w = crate::languages::PersistedWorkspaceHarness::new();
        w.write(path, source);
        w.build();
        let conn = w.open();
        assert_eq!(count(&conn, "SELECT count(*) FROM errors"), 0, "{language}");
        let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
            &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
            Some(100),
            0,
            Some(contextunity_forge_mcp::core::response::Detail::Full),
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
        drop(st);
        let mut expected = vec![("caller".into(), "helper".into())];
        if language == "java" {
            expected.push(("shadow".into(), "helper".into()));
        }
        assert_eq!(calls, expected, "{language}");
        let relation_count: i64 = count(
            &conn,
            "SELECT count(*) FROM edge_occurrences WHERE kind IN('inherits','implements')",
        );
        assert_eq!(relation_count, relations, "{language}");

        let coverage: Vec<(i64, String, String, String)> = conn
            .prepare("SELECT c.line, e.expression, c.status, v.evidence FROM resolution_coverage c JOIN coverage_expressions e ON e.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id ORDER BY c.line, e.expression, c.status, v.evidence")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            coverage
                .iter()
                .map(|(line, expression, status, evidence)| (
                    *line,
                    expression.as_str(),
                    status.as_str(),
                    evidence.as_str()
                ))
                .collect::<Vec<_>>(),
            expected_coverage(language),
            "{language} coverage classifications"
        );

        drop(conn);
        let edited = source.replace("helper", "renamed");
        let edited = if language == "php" {
            edited.replacen("<?php", "<?php\n", 1)
        } else {
            format!("\n{edited}")
        };
        w.write(path, &edited);
        w.delta(&[path]);
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
        let conn = w.open();
        let error_count: i64 = count(&conn, "SELECT count(*) FROM errors");
        assert!(
            error_count > 0,
            "{language} malformed syntax has diagnostics"
        );
    }
}
