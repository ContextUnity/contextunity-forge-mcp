use super::*;
use contextunity_forge_mcp::core::{commitments, semantic::ValueFlowFacts};
use contextunity_forge_mcp::engine::languages::{self, LanguageProfile, SyntaxGrammarSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    time::{Duration, UNIX_EPOCH},
};

fn compacted_details(details: &Value, kind: &str, remove_column: bool) -> Value {
    let Value::Object(details) = details else {
        return details.clone();
    };
    let mut details = details.clone();
    details.retain(|key, value| {
        !((key == "column" && remove_column)
            || (key == "is_method" && kind == "method" && value == &json!(true))
            || ((key == "async"
                || key == "default_export"
                || key == "generic_impl"
                || key == "is_method"
                || key == "is_public"
                || key == "is_static")
                && value == &json!(false))
            || ((key == "bases" || key == "receiver" || key == "receiver_name") && value.is_null())
            || (key == "value_flow" && value.is_null())
            || ((key == "bindings" || key == "rebindings")
                && value.as_array().is_some_and(Vec::is_empty))
            || ((key == "decorators" || key == "bases")
                && value.as_array().is_some_and(Vec::is_empty))
            || ((key == "doc" || key == "receiver" || key == "receiver_name")
                && value.as_str().is_some_and(str::is_empty))
            || (key == "param_types" && value.as_object().is_some_and(serde_json::Map::is_empty)))
    });
    if details.is_empty() {
        Value::Null
    } else {
        Value::Object(details)
    }
}

fn compacted_public_facts(facts: &Value) -> Value {
    let mut facts = facts.clone();
    if let Some(files) = facts.as_object_mut() {
        for facts in files.values_mut() {
            let Some(nodes) = facts["nodes"].as_array_mut() else {
                continue;
            };
            let mut declarations_per_line = std::collections::HashMap::new();
            for node in nodes.iter() {
                if node["kind"] == "module" {
                    continue;
                }
                *declarations_per_line
                    .entry(node["line"].as_u64().unwrap_or_default())
                    .or_insert(0usize) += 1;
            }
            for node in nodes {
                let kind = node["kind"].as_str().unwrap_or_default().to_owned();
                let line = node["line"].as_u64().unwrap_or_default();
                let remove_column = matches!(kind.as_str(), "function" | "method")
                    && declarations_per_line.get(&line) == Some(&1);
                node["details"] = compacted_details(&node["details"], &kind, remove_column);
            }
        }
    }
    facts
}

fn baseline() -> Value {
    serde_json::from_str(include_str!("../fixtures/three_tier/pre_refactor.json")).unwrap()
}

const SOURCES: &[(&str, &[u8])] = &[
    (
        "forge-mcp.yaml",
        include_bytes!("../fixtures/three_tier/forge-mcp.yaml"),
    ),
    (
        "src/helper.py",
        include_bytes!("../fixtures/three_tier/src/helper.py"),
    ),
    (
        "src/helper.rs",
        include_bytes!("../fixtures/three_tier/src/helper.rs"),
    ),
    (
        "src/helper.ts",
        include_bytes!("../fixtures/three_tier/src/helper.ts"),
    ),
    (
        "src/lib.rs",
        include_bytes!("../fixtures/three_tier/src/lib.rs"),
    ),
    (
        "src/main.py",
        include_bytes!("../fixtures/three_tier/src/main.py"),
    ),
    (
        "src/main.ts",
        include_bytes!("../fixtures/three_tier/src/main.ts"),
    ),
];

fn public_facts() -> Value {
    let mut public = serde_json::Map::new();
    for &(path, bytes) in SOURCES {
        let language = match path.rsplit('.').next().unwrap() {
            "py" => "python",
            "rs" => "rust",
            "ts" => "typescript",
            _ => continue,
        };
        let facts = ExtractorHarness::for_lang(language)
            .at_path(path)
            .extract(std::str::from_utf8(bytes).unwrap());
        public.insert(path.into(), serde_json::to_value(facts).unwrap());
    }
    Value::Object(public)
}

fn expected_public_facts() -> Value {
    let baseline = baseline();
    let mut expected = baseline["public_extraction"].clone();
    let caller = expected["src/lib.rs"]["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["id"] == "rs:src/lib.rs:4:caller")
        .unwrap();
    let inner_worker = &mut caller["details"]["value_flow"]["bindings"][2];
    inner_worker.as_object_mut().unwrap().remove("conditional");
    inner_worker["value"] = json!({
        "kind": "scoped",
        "value": {"kind": "unknown"},
        "body_end": {"line": 15, "column": 5}
    });
    let run = expected["src/helper.rs"]["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["id"] == "rs:src/helper.rs:4:run")
        .unwrap();
    run["details"]["value_flow"]["bindings"] = json!([{
        "name": "self",
        "position": {"line": 4, "column": 20},
        "value": {"kind": "annotated", "type_expr": {
            "kind": "applied", "base": "&", "args": [{"kind": "named", "name": "Self"}]
        }}
    }]);
    expected
}

#[test]
fn delivered_baseline_metadata_and_source_bytes_are_bound() {
    let baseline = baseline();
    assert_eq!(baseline["format"], "forge-three-tier-baseline-v1");
    assert_eq!(
        baseline["release"]["t2_commit"],
        "04b11483d0cf180bb0a896c2a2f5ca2818f43bb9"
    );
    assert_eq!(
        baseline["snapshot"]["commitments"],
        baseline["same_root_repeat"]["commitments"]
    );
    assert_eq!(
        baseline["snapshot"]["metadata"],
        baseline["same_root_repeat"]["metadata"]
    );

    for source in baseline["sources"].as_array().unwrap() {
        let path = source["path"].as_str().unwrap();
        let bytes = SOURCES.iter().find(|(name, _)| *name == path).unwrap().1;
        assert_eq!(
            bytes.len() as u64,
            source["size"].as_u64().unwrap(),
            "{path}"
        );
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            source["sha256"].as_str().unwrap(),
            "{path}"
        );
    }
}

#[test]
fn baseline_receiver_adjustments_retain_the_delivered_preconditions() {
    let baseline = baseline();
    let expected = &baseline["public_extraction"];
    let caller = expected["src/lib.rs"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "rs:src/lib.rs:4:caller")
        .unwrap();
    let inner_worker = &caller["details"]["value_flow"]["bindings"][2];
    assert_eq!(inner_worker["position"], json!({"line": 13, "column": 23}));
    assert_eq!(inner_worker["conditional"], true);

    let run = expected["src/helper.rs"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "rs:src/helper.rs:4:run")
        .unwrap();
    assert!(run["details"]["value_flow"]["bindings"].is_null());
}

#[test]
fn public_extraction_preserves_baseline_facts_and_meaningful_details() {
    let expected = expected_public_facts();
    let mut public = public_facts();
    for (path, facts) in public.as_object().unwrap() {
        assert!(
            facts["errors"].as_array().unwrap().is_empty(),
            "{path}: {}",
            facts["errors"]
        );
        let language = match path.rsplit('.').next().unwrap() {
            "py" => "python",
            "rs" => "rust",
            "ts" => "typescript",
            _ => unreachable!(),
        };
        for node in facts["nodes"].as_array().unwrap() {
            assert_eq!(node["path"], path.as_str());
            assert_eq!(node["language"], language);
            assert!(node["line"].as_u64().unwrap() > 0);
            assert!(node["end_line"].as_u64().unwrap() >= node["line"].as_u64().unwrap());
            if let Some(flow) = node["details"].get("value_flow") {
                let flow: ValueFlowFacts = serde_json::from_value(flow.clone()).unwrap();
                assert!(!flow.fact_limit_exceeded, "{path}: {flow:?}");
            }
            assert_ne!(node["details"], json!({}));
        }
    }
    let expected_module_import = json!({
        "source": "module:src/lib.rs",
        "expression": "crate::helper",
        "kind": "imports",
        "line": 1,
        "column": 0,
        "alias": "helper",
        "module": "crate::helper",
        "dynamic": false,
    });
    let module_imports: Vec<_> = public["src/lib.rs"]["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| {
            !expected["src/lib.rs"]["references"]
                .as_array()
                .unwrap()
                .contains(reference)
        })
        .cloned()
        .collect();

    assert_eq!(
        module_imports,
        std::slice::from_ref(&expected_module_import)
    );
    public["src/lib.rs"]["references"]
        .as_array_mut()
        .unwrap()
        .retain(|reference| reference != &expected_module_import);
    assert_eq!(
        compacted_public_facts(&expected),
        compacted_public_facts(&public)
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_references_preserve_order() {
    let facts = ExtractorHarness::for_lang("vue")
        .at_path("order.vue")
        .extract("<script setup>const left = 1; const right = 2;</script>\n<template>{{ left + right }}</template>");
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let references: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.source.starts_with("template_scope:"))
        .map(|reference| reference.expression.as_str())
        .collect();
    assert_eq!(references, ["right", "left"]);
}

fn commitment_fixture() -> PersistedWorkspaceHarness {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for source in baseline()["sources"].as_array().unwrap() {
        let path = source["path"].as_str().unwrap();
        let bytes = SOURCES.iter().find(|(name, _)| *name == path).unwrap().1;
        workspace.write(path, std::str::from_utf8(bytes).unwrap());
        fs::File::options()
            .write(true)
            .open(workspace.path(path))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                UNIX_EPOCH + Duration::from_nanos(source["mtime_ns"].as_u64().unwrap()),
            ))
            .unwrap();
    }
    workspace
}

fn domain_commitments(conn: &rusqlite::Connection) -> Vec<(String, Option<String>, String)> {
    conn.prepare("SELECT domain,p.path,CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id ORDER BY domain,p.path").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
}

#[test]
fn current_commitment_domains_are_stable_across_repeated_real_builds() {
    let workspace = commitment_fixture();
    workspace.build();
    let conn = workspace.open();
    commitments::verify(&conn).unwrap();
    let actual = domain_commitments(&conn);
    let domains: BTreeSet<_> = actual
        .iter()
        .filter(|(_, owner, _)| owner.is_none())
        .map(|(domain, _, _)| domain.as_str())
        .collect();
    assert_eq!(domains, "source_inventory files nodes edges local_facts edge_occurrences dependencies shared_owners errors resolution_coverage coverage_owner_language coverage_language_counts doc_sections node_search_postings node_search_config doc_search_postings doc_search_config".split_whitespace().collect());

    drop(conn);
    workspace.build();
    let repeated = workspace.open();
    commitments::verify(&repeated).unwrap();
    assert_eq!(actual, domain_commitments(&repeated));
}

#[test]
fn rust_simple_and_associated_declarations_keep_compact_classification() {
    let simple = ExtractorHarness::for_lang("rust")
        .at_path("simple.rs")
        .extract("fn simple() {}");
    assert!(simple
        .nodes
        .iter()
        .find(|node| node.name == "simple")
        .unwrap()
        .details
        .is_null());
    let methods = ExtractorHarness::for_lang("rust")
        .at_path("simple_method.rs")
        .extract("struct Simple;\nimpl Simple {\n    fn method() {}\n}\n");
    assert_eq!(
        methods
            .nodes
            .iter()
            .find(|node| node.name == "method")
            .unwrap()
            .details["is_static"],
        true
    );
    let same_line = ExtractorHarness::for_lang("rust")
        .at_path("same_line.rs")
        .extract("fn first() {} fn second() {}");
    assert!(same_line
        .nodes
        .iter()
        .filter(|node| matches!(node.name.as_str(), "first" | "second"))
        .all(|node| node.details["column"].as_u64().is_some()));
}

struct ScopeOrderProfile(std::sync::Mutex<Vec<String>>);
impl contextunity_forge_mcp::engine::languages::LanguageProfile for ScopeOrderProfile {
    fn id(&self) -> &'static str {
        "scope-order"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        contextunity_forge_mcp::engine::languages::require("python")
            .unwrap()
            .grammar(path)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        (kind == "function_definition").then_some("function")
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "scope-order"
    }
    fn normalize_import(
        &self,
        _owner: &str,
        _module: &str,
    ) -> Option<contextunity_forge_mcp::engine::languages::ImportPath> {
        None
    }
    fn value_flow(&self, node: tree_sitter::Node<'_>, source: &str) -> ValueFlowFacts {
        if node.kind() == "function_definition" {
            let name = node
                .child_by_field_name("name")
                .unwrap()
                .utf8_text(source.as_bytes())
                .unwrap();
            self.0.lock().unwrap().push(format!("flow:{name}"));
        }
        ValueFlowFacts::default()
    }
    fn scope_analysis<'source, 'tree>(
        &self,
        node: tree_sitter::Node<'tree>,
        source: &'source str,
        file: &contextunity_forge_mcp::engine::languages::FileContext,
    ) -> Option<ast::ScopeAnalysis<'source, 'tree>> {
        contextunity_forge_mcp::engine::languages::require("python")
            .unwrap()
            .scope_analysis(node, source, file)
    }
    fn extract_calls(
        &self,
        ctx: &contextunity_forge_mcp::engine::languages::SyntaxContext<'_, '_>,
        _facts: &mut contextunity_forge_mcp::core::models::Facts,
    ) {
        if ctx.node.kind() == "call" {
            self.0.lock().unwrap().push(format!(
                "call:{}",
                ctx.node.utf8_text(ctx.source.as_bytes()).unwrap()
            ));
        }
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut contextunity_forge_mcp::core::models::Facts,
    ) -> anyhow::Result<()> {
        contextunity_forge_mcp::engine::languages::parse_file(self, path, source, module, facts)
    }
}

#[test]
fn incremental_scope_analysis_does_not_invoke_legacy_flow_hooks() {
    let profile = ScopeOrderProfile(std::sync::Mutex::new(Vec::new()));
    let mut facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::parse_file(
        &profile,
        "probe.py",
        "def outer():\n    first()\n    def inner():\n        nested()\n    last()\n",
        "probe",
        &mut facts,
    )
    .unwrap();
    let trace = profile.0.into_inner().unwrap();
    let expected = ["call:first()", "call:nested()", "call:last()"];
    assert_eq!(trace, expected);
}

struct DeclarativeProfile(&'static SyntaxGrammarSpec);
impl LanguageProfile for DeclarativeProfile {
    fn id(&self) -> &'static str {
        "declarative-probe"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        languages::require("python").unwrap().grammar(path)
    }
    fn symbol_kind(&self, _kind: &str) -> Option<&'static str> {
        None
    }
    fn syntax_grammar_spec(&self) -> Option<&'static SyntaxGrammarSpec> {
        Some(self.0)
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "declarative"
    }
    fn normalize_import(&self, _owner: &str, _module: &str) -> Option<languages::ImportPath> {
        None
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut contextunity_forge_mcp::core::models::Facts,
    ) -> anyhow::Result<()> {
        languages::parse_file(self, path, source, module, facts)
    }
}
static SYNTAX: SyntaxGrammarSpec = SyntaxGrammarSpec {
    declarations: &[("function_definition", "function")],
    references: &["call"],
    scopes: &["module", "function_definition"],
    imports: &["import_statement"],
    member_access: &["attribute"],
    comments: &["comment"],
};
static INVALID: [SyntaxGrammarSpec; 6] = [
    SyntaxGrammarSpec {
        declarations: &[("unknown_decl", "function")],
        references: &[],
        scopes: &[],
        imports: &[],
        member_access: &[],
        comments: &[],
    },
    SyntaxGrammarSpec {
        declarations: &[],
        references: &["unknown_ref"],
        scopes: &[],
        imports: &[],
        member_access: &[],
        comments: &[],
    },
    SyntaxGrammarSpec {
        declarations: &[],
        references: &[],
        scopes: &["unknown_scope"],
        imports: &[],
        member_access: &[],
        comments: &[],
    },
    SyntaxGrammarSpec {
        declarations: &[],
        references: &[],
        scopes: &[],
        imports: &["unknown_import"],
        member_access: &[],
        comments: &[],
    },
    SyntaxGrammarSpec {
        declarations: &[],
        references: &[],
        scopes: &[],
        imports: &[],
        member_access: &["unknown_member"],
        comments: &[],
    },
    SyntaxGrammarSpec {
        declarations: &[],
        references: &[],
        scopes: &[],
        imports: &[],
        member_access: &[],
        comments: &["unknown_comment"],
    },
];

const SYNTAX_SOURCE: &str =
    "import package\n# profile-doc\ndef sample(arg):\n    local = arg\n    service.execute()\n";

#[test]
fn declarative_syntax_categories_validate_and_emit_facts() {
    let mut syntax_facts = contextunity_forge_mcp::core::models::Facts::default();
    languages::parse_file(
        &DeclarativeProfile(&SYNTAX),
        "syntax.py",
        SYNTAX_SOURCE,
        "syntax",
        &mut syntax_facts,
    )
    .unwrap();
    let declaration = syntax_facts.nodes.iter().find(|node| node.name == "sample");
    for (category, observed) in [
        (
            "declaration",
            declaration.is_some_and(|node| node.kind == "function"),
        ),
        (
            "scope",
            declaration.is_some_and(|node| {
                crate::common::assertions::projected_node_details(&syntax_facts, node)["bindings"]
                    == json!(["arg", "local"])
            }),
        ),
        (
            "comment",
            declaration.is_some_and(|node| node.details["doc"] == "# profile-doc"),
        ),
        (
            "reference",
            syntax_facts.references.iter().any(|reference| {
                reference.kind == "calls" && reference.expression == "service.execute"
            }),
        ),
        (
            "import",
            syntax_facts.references.iter().any(|reference| {
                reference.kind == "imports" && reference.expression == "import package"
            }),
        ),
        (
            "member",
            syntax_facts.references.iter().any(|reference| {
                reference.kind == "reads" && reference.expression == "service.execute"
            }),
        ),
    ] {
        assert!(observed, "SyntaxGrammarSpec {category}: {syntax_facts:?}");
    }
    for spec in &INVALID {
        let mut facts = contextunity_forge_mcp::core::models::Facts::default();
        let error = languages::parse_file(
            &DeclarativeProfile(spec),
            "invalid.py",
            SYNTAX_SOURCE,
            "invalid",
            &mut facts,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown named kind"), "{error}");
        assert!(facts.nodes.is_empty());
        assert!(facts.references.is_empty());
    }
}

#[test]
fn nested_body_bindings_stay_in_their_own_callable_frames() {
    for (language, path, source) in [
        (
            "python",
            "nested.py",
            "def outer():\n    outer_only = object()\n    def inner():\n        inner_only = object()\n        return inner_only\n    callback = lambda: object()\n    return outer_only\n",
        ),
        (
            "rust",
            "nested.rs",
            "fn outer() { let outer_only = Thing::new(); fn inner() { let inner_only = Thing::new(); } let callback = || { let closure_only = Thing::new(); }; }",
        ),
        (
            "typescript",
            "nested.ts",
            "function outer() { const outer_only = new Thing(); function inner() { const inner_only = new Thing(); return inner_only; } const callback = () => { const closure_only = new Thing(); }; return outer_only; }",
        ),
    ] {
        let facts = ExtractorHarness::for_lang(language).at_path(path).extract(source);

        assert!(facts.errors.is_empty(), "{language}: {:?}", facts.errors);
        let flow = |name| {
            let owner = facts.nodes.iter().find(|node| node.name == name).unwrap();
            let details = crate::common::assertions::projected_node_details(&facts, owner);
            serde_json::from_value::<ValueFlowFacts>(details["value_flow"].clone()).unwrap()
        };
        let outer = flow("outer");
        assert!(outer.bindings.iter().any(|binding| binding.name == "outer_only"));
        assert!(
            !outer
                .bindings
                .iter()
                .any(|binding| matches!(binding.name.as_str(), "inner_only" | "closure_only"))
        );
        let inner = flow("inner");
        assert!(inner.bindings.iter().any(|binding| binding.name == "inner_only"));
        assert!(!inner.bindings.iter().any(|binding| binding.name == "outer_only"));
    }
}
