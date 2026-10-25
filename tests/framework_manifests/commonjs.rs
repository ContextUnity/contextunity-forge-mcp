use super::*;

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_require_imports_bind_identifiers_and_destructured_exports() {
    let facts = ast::extract(
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
    let facts = ast::extract("consumer.cjs", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("consumer.cjs".to_owned(), facts)]));
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
    use contextunity_forge_mcp::engine::linker;
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
        let facts = ast::extract("origin.cjs", "javascript", &source).unwrap();
        assert!(facts.errors.is_empty(), "{source}: {:?}", facts.errors);
        let module = facts.nodes.iter().find(|node| node.kind == "module").unwrap();
        let provenance = module.details["commonjs_bindings"].as_array().unwrap();
        assert_eq!(provenance.len(), 1, "{source}");
        assert_eq!(provenance[0]["alias"], "assert");
        assert_eq!(provenance[0]["module"], "node:assert/strict");
        assert_eq!(provenance[0]["invalidated"], invalidated, "{source}");
        assert_eq!(provenance[0]["captured_safe"], prefix.is_empty(), "{source}");
        let graph = linker::link(&BTreeMap::from([("origin.cjs".into(), facts)]));
        let coverage = graph.coverage.iter().find(|item| item.expression == "assert.equal" && item.line == source.lines().position(|line| line.contains("assert.equal(")).unwrap() + 1).unwrap();
        assert_eq!(coverage.status, expected_status, "{source}: {}", coverage.evidence);
        if expected_status == "external" {
            assert!(coverage.evidence.contains("node:assert.strict"), "{}", coverage.evidence);
        }
    }
    let facts = ast::extract("chain.cjs", "javascript", "const assert = require('node:assert');\nfunction run() { assert.strict.equal(1, 1); }\nrun();").unwrap();
    let module = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    assert_eq!(module.details["commonjs_bindings"][0]["invalidated"], false);
    let graph = linker::link(&BTreeMap::from([("chain.cjs".into(), facts)]));
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
        let facts = ast::extract("scope.cjs", "javascript", source).unwrap();
        let module = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module")
            .unwrap();
        assert_eq!(module.details["commonjs_bindings"], serde_json::json!([]));
        let graph = linker::link(&BTreeMap::from([("scope.cjs".into(), facts)]));
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
    use contextunity_forge_mcp::core::commitments;
    let workspace = Workspace::new();
    let declaration = "const assert = require('node:assert/strict');\n";
    workspace.write(
        "origin.cjs",
        &format!("{declaration}function run() {{ assert.equal(1, 1); }}\nrun();\n"),
    );
    let db = workspace.0.join(".forge/commonjs.db");
    writer::build(&workspace.0, &db, None).unwrap();
    let check = |db: &std::path::Path, expected_status: &str, invalidated: bool| {
        let conn = reader::open(db, &workspace.0).unwrap();
        let details: String = conn
            .query_row(
                "SELECT details FROM nodes WHERE id='module:origin.cjs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let details: serde_json::Value = serde_json::from_str(&details).unwrap();
        assert_eq!(details["commonjs_bindings"][0]["invalidated"], invalidated);
        let status: String = conn.query_row("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='origin.cjs' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='assert.equal'", [], |row| row.get(0)).unwrap();
        assert_eq!(status, expected_status);
        commitments::verify(&conn).unwrap();
    };
    check(&db, "external", false);
    workspace.write("origin.cjs", &format!("{declaration}assert.equal = custom;\nfunction run() {{ assert.equal(1, 1); }}\nrun();\n"));
    writer::delta(&workspace.0, &db, &[PathBuf::from("origin.cjs")]).unwrap();
    check(&db, "unresolved", true);
    let cold = workspace.0.join(".forge/commonjs-cold.db");
    writer::build(&workspace.0, &cold, None).unwrap();
    check(&cold, "unresolved", true);
    let incremental = reader::open(&db, &workspace.0).unwrap();
    let cold = reader::open(&cold, &workspace.0).unwrap();
    for sql in [
        "SELECT path||'|'||hex(facts_blob) FROM local_facts ORDER BY path",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        "SELECT p.path||'|'||c.domain||'|'||hex(c.digest) FROM domain_commitments c JOIN path_dictionary p ON p.path_id=c.owner_id ORDER BY p.path,c.domain",
    ] {
        assert_eq!(query_rows(&incremental, sql), query_rows(&cold, sql), "{sql}");
    }
}
#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_provider_body_edits_preserve_unchanged_consumer_contracts() {
    use contextunity_forge_mcp::core::commitments;
    let workspace = Workspace::new();
    let provider = |checkpoint| {
        format!("const assert = require('node:assert/strict');\nfunction work(value) {{\n  const checkpoint = {checkpoint};\n  assert.equal(value, value);\n  return value;\n}}\nmodule.exports = {{ work }};\n")
    };
    let before_source = provider(1);
    let after_source = provider(2000);
    let metadata = |source: &str| {
        ast::extract("provider.cjs", "javascript", source)
            .unwrap()
            .nodes
            .into_iter()
            .find(|node| node.kind == "module")
            .unwrap()
            .details["commonjs_bindings"]
            .clone()
    };
    assert_eq!(metadata(&before_source), metadata(&after_source));
    workspace.write("provider.cjs", &before_source);
    workspace.write(
        "consumer.cjs",
        "const provider = require('./provider');\nfunction run() { provider.work(1); }\nrun();\n",
    );
    let db = workspace.0.join(".forge/commonjs-body.db");
    writer::build(&workspace.0, &db, None).unwrap();
    let snapshot = |db: &std::path::Path| {
        let conn = reader::open(db, &workspace.0).unwrap();
        commitments::verify(&conn).unwrap();
        let mut statement = conn.prepare("SELECT domain||'|'||hex(digest) FROM domain_commitments WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.cjs') ORDER BY domain").unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    let consumer_before = snapshot(&db);
    workspace.write("provider.cjs", &after_source);
    let report = writer::delta(&workspace.0, &db, &[PathBuf::from("provider.cjs")]).unwrap();
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(snapshot(&db), consumer_before);
    let cold = workspace.0.join(".forge/commonjs-body-cold.db");
    writer::build(&workspace.0, &cold, None).unwrap();
    assert_eq!(snapshot(&db), snapshot(&cold));
}
