use super::*;

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_require_imports_bind_identifiers_and_destructured_exports() {
    let facts = FrameworkHarness::extract(
        "sample.cjs",
        "javascript",
        "const fs = require('fs');\nconst { readFile, writeFile: write } = require('node:fs/promises');\nfunction run(require) { const local = require('not-a-module'); }\n",
    )
    .unwrap();
    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == "imports")
        .collect();
    assert_eq!(imports.len(), 3, "{imports:?}");
    assert!(imports.iter().any(|r| r.expression == "*"
        && r.alias.as_deref() == Some("fs")
        && r.module.as_deref() == Some("fs")));
    assert!(imports.iter().any(|r| r.expression == "readFile"
        && r.alias.as_deref() == Some("readFile")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(imports.iter().any(|r| r.expression == "writeFile"
        && r.alias.as_deref() == Some("write")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(!imports
        .iter()
        .any(|r| r.module.as_deref() == Some("not-a-module")));
}
#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_destructuring_default_reads_invalidate_import_provenance() {
    let source = "const { spare = require } = require('node:assert/strict');\nspare('x');\n";
    let facts = FrameworkHarness::extract("consumer.cjs", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = FrameworkHarness::link(&BTreeMap::from([("consumer.cjs".to_owned(), facts)]));
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.line == 2 && row.expression == "spare")
        .unwrap_or_else(|| panic!("missing call coverage: {graph:#?}"));
    assert_eq!(coverage.status, "unresolved", "{coverage:#?}");
}
#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_namespace_origins_require_completed_stable_module_bindings() {
    use std::collections::BTreeMap;

    let declaration = "const assert = require('node:assert/strict');\n";
    for (prefix, body, expected_status, invalidated) in [
        ("", "assert.equal(1, 1);", "external", false),
        ("", "function run() { assert.equal(1, 1); } run();", "external", false),
        ("", "function local(assert) { assert = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("", "function local(require) { require = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("", "for (let assert of values) { assert.equal = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("run();\n", "function run() { assert.equal(1, 1); }", "unresolved", false),
        ("require = custom;\n", "assert.equal(1, 1);", "unresolved", true),
        ("assert.equal(1, 1);\n", "", "unresolved", false),
        ("", "function run(assert) { assert.equal(1, 1); }", "unresolved", false),
        ("", "function run() { const assert = custom; assert.equal(1, 1); }", "unresolved", false),
        ("", "assert = custom; assert.equal(1, 1);", "unresolved", true),
        ("", "assert.equal = custom; assert.equal(1, 1);", "unresolved", true),
        ("", "[assert.equal] = [custom]; assert.equal(1, 1);", "unresolved", true),
        ("", "for (assert.equal of [custom]) {} assert.equal(1, 1);", "unresolved", true),
        ("", "assert++; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = assert; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = () => assert; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = { assert }; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = [assert]; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = (assert); assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = assert.strict; escaped.equal = custom; assert.equal(1, 1);", "unresolved", true),
    ] {
        let source = format!("{prefix}{declaration}{body}");
        let facts = FrameworkHarness::extract("origin.cjs", "javascript", &source).unwrap();
        assert!(facts.errors.is_empty(), "{source}: {:?}", facts.errors);
        let module = facts.nodes.iter().find(|node| node.kind == "module").unwrap();
        let provenance = module.details["commonjs_bindings"].as_array().unwrap();
        assert_eq!(provenance.len(), 1, "{source}");
        assert_eq!(provenance[0]["alias"], "assert");
        assert_eq!(provenance[0]["module"], "node:assert/strict");
        assert_eq!(provenance[0]["invalidated"], invalidated, "{source}");
        assert_eq!(provenance[0]["captured_safe"], prefix.is_empty(), "{source}");
        let graph = FrameworkHarness::link(&BTreeMap::from([("origin.cjs".into(), facts)]));
        let coverage = graph.coverage.iter().find(|item| item.expression == "assert.equal" && item.line == source.lines().position(|line| line.contains("assert.equal(")).unwrap() + 1).unwrap();
        assert_eq!(coverage.status, expected_status, "{source}: {}", coverage.evidence);
        if expected_status == "external" {
            assert!(coverage.evidence.contains("node:assert.strict"), "{}", coverage.evidence);
        }
    }
    let facts = FrameworkHarness::extract("chain.cjs", "javascript", "const assert = require('node:assert');\nfunction run() { assert.strict.equal(1, 1); }\nrun();").unwrap();
    let module = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    assert_eq!(module.details["commonjs_bindings"][0]["invalidated"], false);
    let graph = FrameworkHarness::link(&BTreeMap::from([("chain.cjs".into(), facts)]));
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|item| item.expression == "assert.strict.equal")
            .unwrap()
            .status,
        "external"
    );
    for source in [
        "function run() { const assert = require('node:assert/strict'); assert.equal(1, 1); }",
        "if (enabled) { const assert = require('node:assert/strict'); assert.equal(1, 1); }",
        "let assert = require('node:assert/strict'); assert.equal(1, 1);",
    ] {
        let facts = FrameworkHarness::extract("scope.cjs", "javascript", source).unwrap();
        let module = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module")
            .unwrap();
        assert_eq!(module.details["commonjs_bindings"], serde_json::json!([]));
        let graph = FrameworkHarness::link(&BTreeMap::from([("scope.cjs".into(), facts)]));
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|item| item.expression == "assert.equal")
                .unwrap()
                .status,
            "unresolved"
        );
    }
}
#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_binding_provenance_persists_and_delta_matches_cold() {
    let declaration = "const assert = require('node:assert/strict');\n";
    let before_source = format!("{declaration}function run() {{ assert.equal(1, 1); }}\nrun();\n");
    let after_source = format!(
        "{declaration}assert.equal = custom;\nfunction run() {{ assert.equal(1, 1); }}\nrun();\n"
    );
    let workspace = FrameworkHarness::published(&[("origin.cjs", &before_source)]);
    let snapshot = |conn: &Connection| {
        let details: String = conn
            .query_row(
                "SELECT details FROM nodes WHERE id=?1",
                ["module:origin.cjs"],
                |row| row.get(0),
            )
            .unwrap();
        let rows = (
            query_rows(conn, "SELECT path||'|'||hex(facts_blob) FROM local_facts ORDER BY path"),
            query_rows(
                conn,
                "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
            ),
            query_rows(
                conn,
                "SELECT p.path||'|'||c.domain||'|'||hex(c.digest) FROM domain_commitments c JOIN path_dictionary p ON p.path_id=c.owner_id ORDER BY p.path,c.domain",
            ),
        );
        (
            commitments::verify(conn).is_ok(),
            serde_json::from_str::<Value>(&details).unwrap(),
            coverage_status(conn, "origin.cjs", "assert.equal"),
            rows,
        )
    };
    let before = snapshot(&workspace.open());
    workspace.write("origin.cjs", &after_source);
    workspace.delta(&["origin.cjs"]);
    let incremental = snapshot(&workspace.open());
    let cold_workspace = FrameworkHarness::published(&[("origin.cjs", &after_source)]);
    let cold = snapshot(&cold_workspace.open());
    assert_eq!(before.1["commonjs_bindings"][0]["invalidated"], false);
    assert_eq!(before.2, "external");
    assert!(before.0);
    assert_eq!(incremental.1["commonjs_bindings"][0]["invalidated"], true);
    assert_eq!(incremental.2, "unresolved");
    assert!(incremental.0);
    assert_eq!(cold.1["commonjs_bindings"][0]["invalidated"], true);
    assert_eq!(cold.2, "unresolved");
    assert!(cold.0);
    assert_eq!(
        incremental.3, cold.3,
        "complete compressed local facts, coverage and owner commitments cold/delta parity"
    );
}
#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_provider_body_edits_preserve_unchanged_consumer_contracts() {
    let provider = |checkpoint| {
        format!("const assert = require('node:assert/strict');\nfunction work(value) {{\n  const checkpoint = {checkpoint};\n  assert.equal(value, value);\n  return value;\n}}\nmodule.exports = {{ work }};\n")
    };
    let before_source = provider(1);
    let after_source = provider(2000);
    let metadata = |source: &str| {
        FrameworkHarness::extract("provider.cjs", "javascript", source)
            .unwrap()
            .nodes
            .into_iter()
            .find(|node| node.kind == "module")
            .unwrap()
            .details["commonjs_bindings"]
            .clone()
    };
    assert_eq!(metadata(&before_source), metadata(&after_source));
    let consumer_source =
        "const provider = require('./provider');\nfunction run() { provider.work(1); }\nrun();\n";
    let workspace = FrameworkHarness::published(&[
        ("provider.cjs", &before_source),
        ("consumer.cjs", consumer_source),
    ]);
    let snapshot = |conn: &Connection| {
        (
            commitments::verify(conn).is_ok(),
            owner_commitments(conn, "consumer.cjs"),
        )
    };
    let before = snapshot(&workspace.open());
    workspace.write("provider.cjs", &after_source);
    let report = workspace.delta(&["provider.cjs"]);
    let incremental = snapshot(&workspace.open());
    let cold_workspace = FrameworkHarness::published(&[
        ("provider.cjs", &after_source),
        ("consumer.cjs", consumer_source),
    ]);
    let cold = snapshot(&cold_workspace.open());
    assert!(before.0);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert!(incremental.0);
    assert!(cold.0);
    assert_eq!(incremental.1, before.1);
    assert_eq!(incremental.1, cold.1);
}
