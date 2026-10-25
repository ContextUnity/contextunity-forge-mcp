use super::support::Workspace;
use contextunity_forge_mcp::db::{reader, writer};
use contextunity_forge_mcp::engine::ast;
use std::{fs, path::PathBuf};

#[cfg(feature = "lang-typescript")]
mod legacy_hooks;

#[cfg(all(feature = "lang-python", feature = "lang-rust"))]
#[test]
fn syntax_grammar_cache_isolated_by_spec_and_actual_grammar() {
    use contextunity_forge_mcp::core::models::Facts;
    use contextunity_forge_mcp::engine::languages::{self, LanguageProfile, SyntaxGrammarSpec};
    static FUNCTION_SPEC: SyntaxGrammarSpec = SyntaxGrammarSpec {
        declarations: &[("function_definition", "function")],
        ..SyntaxGrammarSpec {
            references: &[],
            scopes: &[],
            imports: &[],
            member_access: &[],
            comments: &[],
            declarations: &[],
        }
    };
    static CLASS_SPEC: SyntaxGrammarSpec = SyntaxGrammarSpec {
        declarations: &[("class_definition", "class")],
        ..SyntaxGrammarSpec {
            references: &[],
            scopes: &[],
            imports: &[],
            member_access: &[],
            comments: &[],
            declarations: &[],
        }
    };

    struct Probe(&'static SyntaxGrammarSpec, &'static str);
    impl LanguageProfile for Probe {
        fn id(&self) -> &'static str {
            self.1
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["py", "rs"]
        }
        fn grammar(&self, path: &str) -> tree_sitter::Language {
            if path.ends_with(".rs") {
                languages::require("rust").unwrap().grammar(path)
            } else {
                languages::require("python").unwrap().grammar(path)
            }
        }
        fn symbol_kind(&self, _kind: &str) -> Option<&'static str> {
            None
        }
        fn syntax_grammar_spec(&self) -> Option<&'static SyntaxGrammarSpec> {
            Some(self.0)
        }
        fn node_prefix(&self, _kind: &str) -> &'static str {
            "syntax-cache"
        }
        fn normalize_import(&self, _owner: &str, _module: &str) -> Option<languages::ImportPath> {
            None
        }
        fn extract_file(
            &self,
            path: &str,
            source: &str,
            module: &str,
            facts: &mut Facts,
        ) -> anyhow::Result<()> {
            languages::parse_file(self, path, source, module, facts)
        }
    }

    let source = "def run():\n    pass\n\nclass Thing:\n    pass\n";
    let mut function_facts = Facts::default();
    languages::parse_file(
        &Probe(&FUNCTION_SPEC, "function-probe"),
        "probe.py",
        source,
        "probe",
        &mut function_facts,
    )
    .unwrap();
    assert!(function_facts
        .nodes
        .iter()
        .any(|node| node.name == "run" && node.kind == "function"));

    let mut class_facts = Facts::default();
    languages::parse_file(
        &Probe(&CLASS_SPEC, "class-probe"),
        "probe.py",
        source,
        "probe",
        &mut class_facts,
    )
    .unwrap();
    assert!(class_facts
        .nodes
        .iter()
        .any(|node| node.name == "Thing" && node.kind == "class"));

    let wrong_grammar = languages::parse_file(
        &Probe(&FUNCTION_SPEC, "wrong-grammar-probe"),
        "probe.rs",
        "fn run() {}",
        "probe",
        &mut Facts::default(),
    );
    assert!(wrong_grammar.is_err());
}

#[cfg(all(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]
#[test]
fn three_tier_profiles_preserve_semantics_and_current_commitments() {
    use contextunity_forge_mcp::core::commitments;
    use contextunity_forge_mcp::core::semantic::ValueFlowFacts;
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
            if (key == "column" && remove_column)
                || (key == "is_method" && kind == "method" && value == &json!(true))
                || ((key == "async"
                    || key == "default_export"
                    || key == "generic_impl"
                    || key == "is_method"
                    || key == "is_public"
                    || key == "is_static")
                    && value == &json!(false))
                || ((key == "bases" || key == "receiver" || key == "receiver_name")
                    && value.is_null())
                || (key == "value_flow" && value.is_null())
                || ((key == "bindings" || key == "rebindings")
                    && value.as_array().is_some_and(Vec::is_empty))
                || ((key == "decorators" || key == "bases")
                    && value.as_array().is_some_and(Vec::is_empty))
                || ((key == "doc" || key == "receiver" || key == "receiver_name")
                    && value.as_str().is_some_and(str::is_empty))
                || (key == "param_types"
                    && value.as_object().is_some_and(serde_json::Map::is_empty))
            {
                return false;
            }
            true
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

    let baseline: Value =
        serde_json::from_str(include_str!("fixtures/three_tier/pre_refactor.json")).unwrap();
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
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/languages/fixtures/three_tier");
    let workspace = Workspace::new();
    let mut public = serde_json::Map::new();
    let mut failures = Vec::new();
    for source in baseline["sources"].as_array().unwrap() {
        let path = source["path"].as_str().unwrap();
        let bytes = fs::read(fixture_root.join(path)).unwrap();
        assert_eq!(
            bytes.len() as u64,
            source["size"].as_u64().unwrap(),
            "{path}"
        );
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            source["sha256"].as_str().unwrap(),
            "{path}"
        );
        let text = std::str::from_utf8(&bytes).unwrap();
        workspace.write(path, text);
        fs::File::options()
            .write(true)
            .open(workspace.0.join(path))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                UNIX_EPOCH + Duration::from_nanos(source["mtime_ns"].as_u64().unwrap()),
            ))
            .unwrap();
        let language = match path.rsplit('.').next().unwrap() {
            "py" => "python",
            "rs" => "rust",
            "ts" => "typescript",
            _ => continue,
        };
        let facts = ast::extract(path, language, text).unwrap();
        if !facts.errors.is_empty() {
            failures.push(format!("{path}: {:?}", facts.errors));
        }
        let projected = crate::common::assertions::projected_facts(&facts);
        for (node, public_node) in facts
            .nodes
            .iter()
            .zip(projected["nodes"].as_array().unwrap())
        {
            if node.path != path
                || node.language != language
                || node.line == 0
                || node.end_line < node.line
            {
                failures.push(format!("invalid public owner: {node:?}"));
            }
            if let Some(flow) = public_node["details"].get("value_flow") {
                let typed: ValueFlowFacts = serde_json::from_value(flow.clone()).unwrap();
                if typed.fact_limit_exceeded {
                    failures.push(format!("unexpected bounded fact overflow: {}", node.id));
                }
            }
            if public_node["details"] == json!({}) {
                failures.push(format!("empty details object is not compact: {}", node.id));
            }
        }
        public.insert(path.into(), serde_json::to_value(&facts).unwrap());
    }
    // All owners are captured through the public API before any comparison can
    // fail, so one unsupported assumption cannot hide another language's facts.
    let expected = baseline
        .get("public_extraction")
        .expect("baseline requires delivered public owner/binding/return prevalidation");
    for (path, facts) in &public {
        for node in facts["nodes"].as_array().unwrap() {
            let original = expected[path]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|original| original["id"] == node["id"]);
            let kind = node["kind"].as_str().unwrap_or_default();
            let line = node["line"].as_u64().unwrap_or_default();
            let declarations_on_line = facts["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|candidate| {
                    candidate["kind"] != "module" && candidate["line"].as_u64() == Some(line)
                })
                .count();
            let remove_column = matches!(kind, "function" | "method") && declarations_on_line == 1;
            if let Some(original) = original {
                let expected = compacted_details(&original["details"], kind, remove_column);
                let actual = compacted_details(&node["details"], kind, remove_column);
                if expected != actual {
                    let brief = |value: &Value| {
                        serde_json::to_string(value)
                            .unwrap()
                            .chars()
                            .take(320)
                            .collect::<String>()
                    };
                    failures.push(format!(
                        "{path}: meaningful details changed for {}: expected {}, got {}",
                        node["id"],
                        brief(&expected),
                        brief(&actual),
                    ));
                }
            }
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
    if module_imports != [expected_module_import.clone()] {
        failures.push(format!(
            "unexpected additive Rust module facts: {module_imports:?}"
        ));
    }
    let mut comparable_public = Value::Object(public.clone());
    comparable_public["src/lib.rs"]["references"]
        .as_array_mut()
        .unwrap()
        .retain(|reference| reference != &expected_module_import);
    let compacted_expected = compacted_public_facts(expected);
    let compacted_actual = compacted_public_facts(&comparable_public);
    if compacted_expected != compacted_actual {
        for (path, expected_facts) in compacted_expected.as_object().unwrap() {
            let actual_facts = &compacted_actual[path];
            for (field, expected_value) in expected_facts.as_object().unwrap() {
                if field == "nodes" {
                    let without_details = |nodes: &Value| {
                        Value::Array(
                            nodes
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|node| {
                                    let mut node = node.clone();
                                    node.as_object_mut().unwrap().remove("details");
                                    node
                                })
                                .collect(),
                        )
                    };
                    if without_details(expected_value) != without_details(&actual_facts[field]) {
                        failures.push(format!("{path}: non-detail node facts changed"));
                    }
                } else if expected_value != &actual_facts[field] {
                    failures.push(format!(
                        "{path}: {field} changed: expected {expected_value:?}, got {:?}",
                        actual_facts[field]
                    ));
                }
            }
        }
    }
    #[cfg(feature = "lang-vue")]
    {
        let facts = ast::extract(
            "order.vue",
            "vue",
            "<script setup>const left = 1; const right = 2;</script>\n<template>{{ left + right }}</template>",
        )
        .unwrap();
        assert!(facts.errors.is_empty(), "{:?}", facts.errors);
        let references: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.source.starts_with("template_scope:"))
            .map(|reference| reference.expression.as_str())
            .collect();
        assert_eq!(references, ["right", "left"]);
    }
    let conn = workspace.build();
    commitments::verify(&conn).unwrap();
    let mut statement = conn.prepare("SELECT domain,p.path,CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id ORDER BY domain,p.path").unwrap();
    let actual: Vec<(String, Option<String>, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let domains: BTreeSet<_> = actual
        .iter()
        .filter(|(_, owner, _)| owner.is_none())
        .map(|(domain, _, _)| domain.as_str())
        .collect();
    assert_eq!(domains, "source_inventory files nodes edges local_facts edge_occurrences dependencies shared_owners errors resolution_coverage coverage_owner_language coverage_language_counts doc_sections node_search_postings node_search_config doc_search_postings doc_search_config".split_whitespace().collect());
    drop(statement);
    drop(conn);
    let repeated = workspace.build();
    commitments::verify(&repeated).unwrap();
    let mut statement = repeated.prepare("SELECT domain,p.path,CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id ORDER BY domain,p.path").unwrap();
    let repeated_commitments: Vec<(String, Option<String>, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    if actual != repeated_commitments {
        failures.push("current 17-domain commitments changed on a repeated build".into());
    }
    let simple = ast::extract("simple.rs", "rust", "fn simple() {}").unwrap();
    if simple
        .nodes
        .iter()
        .find(|node| node.name == "simple")
        .is_none_or(|node| !node.details.is_null())
    {
        failures.push("simple Rust function details must be null".into());
    }
    let simple_method = ast::extract(
        "simple_method.rs",
        "rust",
        "struct Simple;\nimpl Simple {\n    fn method() {}\n}\n",
    )
    .unwrap();
    if simple_method
        .nodes
        .iter()
        .find(|node| node.name == "method")
        .is_none_or(|node| node.details["is_static"] != true)
    {
        failures.push("Rust associated methods retain their static classification".into());
    }
    let same_line = ast::extract("same_line.rs", "rust", "fn first() {} fn second() {}").unwrap();
    if same_line
        .nodes
        .iter()
        .filter(|node| matches!(node.name.as_str(), "first" | "second"))
        .any(|node| node.details["column"].as_u64().is_none())
    {
        failures.push("same-line declaration positions must remain available".into());
    }
    // This profile uses the supported public hook surface. Body summaries must
    // be finalized after their own descendants while retaining nested owners.
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
    if trace != expected {
        failures.push(format!(
            "incremental analysis must not invoke legacy flow hooks: expected {expected:?}, got {trace:?}"
        ));
    }
    // A profile declaring only syntax data executes every category through the
    // public parser. Category removal must change an observable fact.
    use contextunity_forge_mcp::engine::languages::{self, LanguageProfile, SyntaxGrammarSpec};
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
    let syntax_source =
        "import package\n# profile-doc\ndef sample(arg):\n    local = arg\n    service.execute()\n";
    let mut syntax_facts = contextunity_forge_mcp::core::models::Facts::default();
    languages::parse_file(
        &DeclarativeProfile(&SYNTAX),
        "syntax.py",
        syntax_source,
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
        if !observed {
            failures.push(format!(
                "SyntaxGrammarSpec {category} outcome missing: {syntax_facts:?}"
            ));
        }
    }
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
    for spec in &INVALID {
        let mut facts = contextunity_forge_mcp::core::models::Facts::default();
        match languages::parse_file(
            &DeclarativeProfile(spec),
            "invalid.py",
            syntax_source,
            "invalid",
            &mut facts,
        ) {
            Err(error)
                if error.to_string().contains("unknown named kind")
                    && facts.nodes.is_empty()
                    && facts.references.is_empty() => {}
            result => failures.push(format!(
                "invalid SyntaxGrammarSpec must fail before extraction: {result:?}, {facts:?}"
            )),
        }
    }
    // Nested body writes belong only to their own frame, including anonymous
    // callable scopes which are not direct sibling declarations.
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
        let facts = ast::extract(path, language, source).unwrap();
        if !facts.errors.is_empty() {
            failures.push(format!(
                "{language} nested source parse errors: {:?}",
                facts.errors
            ));
        }
        let outer = facts
            .nodes
            .iter()
            .find(|node| node.name == "outer")
            .unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, outer);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
        if !flow
            .bindings
            .iter()
            .any(|binding| binding.name == "outer_only")
            || flow
                .bindings
                .iter()
                .any(|binding| matches!(binding.name.as_str(), "inner_only" | "closure_only"))
        {
            failures.push(format!("{language} nested frame binding leak: {flow:?}"));
        }
        let inner = facts
            .nodes
            .iter()
            .find(|node| node.name == "inner")
            .unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, inner);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
        if !flow
            .bindings
            .iter()
            .any(|binding| binding.name == "inner_only")
            || flow
                .bindings
                .iter()
                .any(|binding| binding.name == "outer_only")
        {
            failures.push(format!("{language} nested owner lost bindings: {flow:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

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
fn root_cargo_crate_imports_link_integration_tests_and_preserve_delta_parity() {
    for (manifest, crate_name, library, expected) in [
        ("[package]\nname='root_fixture'\n", "root_fixture", true, 5),
        ("[package]\nname='root-fixture'\n", "root_fixture", true, 5),
        (
            "[package]\nname='root-fixture'\n[lib]\nname='custom_lib'\n",
            "custom_lib",
            true,
            5,
        ),
        (
            "[package]\nname='root-fixture'\n[lib]\nname='custom_lib'\n",
            "root_fixture",
            true,
            0,
        ),
        ("[package]\nname='root-fixture'\n", "other_fixture", true, 0),
        (
            "[package]\nname='root-fixture'\n",
            "root_fixture_extra",
            true,
            0,
        ),
        ("[workspace]\nmembers=[]\n", "root_fixture", true, 0),
        ("[package\nname='root-fixture'\n", "root_fixture", true, 0),
        ("[package]\nname=[]\n", "root_fixture", true, 0),
        ("", "root_fixture", true, 0),
        ("[package]\nname='root-fixture'\n", "root_fixture", false, 0),
        (
            "[package]\nname='root-fixture'\n[lib]\npath='actual/lib.rs'\n",
            "root_fixture",
            true,
            0,
        ),
        (
            "[package]\nname='root-fixture'\nautolib=false\n",
            "root_fixture",
            true,
            0,
        ),
        (
            "[package]\nname='root-fixture'\nautolib=false\n[lib]\npath='src/lib.rs'\n",
            "root_fixture",
            true,
            5,
        ),
        (
            "[package]\nname='root-fixture'\nautolib=false\n[lib]\n",
            "root_fixture",
            true,
            5,
        ),
    ] {
        let w = Workspace::new();
        if !manifest.is_empty() {
            w.write("Cargo.toml", manifest);
        }
        w.write(
            if library { "src/lib.rs" } else { "src/main.rs" },
            "pub mod api;\npub fn root_route() {}\n",
        );
        w.write("src/api.rs", "pub fn route() -> usize { 1 }\n");
        if manifest.contains("actual/lib.rs") {
            w.write("actual/lib.rs", "pub fn actual() {}\n");
        }
        w.write(
            "tests/connected.rs",
            &format!("use {crate_name}::api::route;\nuse {crate_name}::api::route as alias;\nuse {crate_name}::api;\nuse {crate_name}::root_route;\n#[test]\nfn direct() {{ let actual = route(); assert_eq!(actual, 1); }}\n#[test]\nfn aliased() {{ alias(); }}\n#[test]\nfn namespaced() {{ api::route(); }}\n#[test]\nfn root_symbol() {{ root_route(); }}\nmod glob {{ use {crate_name}::api::*; #[test] fn globbed() {{ route(); }} }}\n"),
        );
        let connection = w.build();
        let calls: i64 = connection.query_row(
            "SELECT count(*) FROM edges edge JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='calls' AND caller.is_test=1 AND caller_path.path='tests/connected.rs' AND target.name IN ('route', 'root_route') AND target_path.path IN ('src/api.rs', 'src/lib.rs', 'src/main.rs')",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            calls, expected,
            "Cargo import {crate_name}, library={library}, manifest={manifest}"
        );
        drop(connection);
        w.write("src/api.rs", "pub fn route() -> usize { 2 }\n");
        w.cold_parity("src/api.rs");
        if expected > 0 {
            w.write("Cargo.toml", "[package]\nname='renamed-fixture'\n");
            w.cold_parity("Cargo.toml");
            let connection = w.open();
            let stale: i64 = connection.query_row(
                "SELECT count(*) FROM edges edge JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash WHERE edge.kind='calls' AND caller.is_test=1 AND target.name IN ('route', 'root_route')",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                stale, 0,
                "Cargo rename invalidates the former crate identity"
            );
        }
    }

    let w = Workspace::new();
    w.write("Cargo.toml", "[package]\nname='root-fixture'\n");
    w.write("src/lib.rs", "pub fn route() {}\n");
    w.write("nested/Cargo.toml", "[package]\nname='nested-fixture'\n");
    w.write(
        "nested/tests/connected.rs",
        "use root_fixture::route;\n#[test]\nfn connected() { route(); }\n",
    );
    let connection = w.build();
    let calls: i64 = connection.query_row(
        "SELECT count(*) FROM edges edge JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash WHERE edge.kind='calls' AND caller.name='connected' AND caller.is_test=1 AND target.name='route'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        calls, 0,
        "a nested Cargo package cannot borrow the root crate identity"
    );
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

#[cfg(feature = "lang-rust")]
#[test]
fn rust_named_forwarding_and_root_library_bindings_survive_delta() {
    use contextunity_forge_mcp::core::commitments;
    use rusqlite::{params, Connection};
    type Probe<'a> = (&'a str, &'a str, Option<(&'a str, &'a str)>, &'a str);

    let mut failures = Vec::new();
    let observe = |conn: &Connection,
                   phase: &str,
                   probes: &[Probe<'_>],
                   failures: &mut Vec<String>| {
        commitments::verify(conn).unwrap();
        let errors: i64 = conn
            .query_row("SELECT count(*) FROM errors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(errors, 0, "{phase}: fixture must parse");
        for &(owner, expression, target, status) in probes {
            let coverage: Vec<(usize, String)> = conn.prepare("SELECT c.line,c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.line,c.status").unwrap()
                .query_map(params![owner, expression], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            // Acceptance3 permits unresolved/ambiguous evidence for a nonexact
            // namespace export; direct named imports retain their strict A oracle.
            let expected_statuses =
                if phase == "missing becomes a distinct target" && expression == "api::partial" {
                    &["unresolved", "ambiguous"][..]
                } else {
                    std::slice::from_ref(&status)
                };
            if coverage.is_empty()
                || coverage
                    .iter()
                    .any(|(_, actual)| !expected_statuses.contains(&actual.as_str()))
            {
                failures.push(format!(
                    "{phase}: {owner} {expression} coverage {coverage:?}, expected {expected_statuses:?}"
                ));
            }
            let mut actual = Vec::<String>::new();
            for (line, _) in &coverage {
                actual.extend(conn.prepare("SELECT n.id FROM edge_occurrences o JOIN nodes n ON n.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path=?1 AND o.line=?2 AND o.kind='calls' ORDER BY n.id").unwrap()
                    .query_map(params![owner, line], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<String>>>().unwrap());
            }
            actual.sort();
            actual.dedup();
            let expected: Vec<String> = target.map(|(path, name)| {
                let ids: Vec<String> = conn.prepare("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 AND n.kind IN ('function','method') ORDER BY n.id").unwrap()
                    .query_map(params![path, name], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(ids.len(), 1, "{phase}: canonical target {path}:{name}");
                ids[0].clone()
            }).into_iter().collect();
            if actual != expected {
                failures.push(format!(
                    "{phase}: {owner} {expression} calls {actual:?}, expected {expected:?}"
                ));
            }
            let dependencies: Vec<String> = conn.prepare("SELECT DISTINCT f.path FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE p.path=?1 AND d.symbol=?2 AND d.kind='calls' AND d.resolution='resolved' ORDER BY f.path").unwrap()
                .query_map(params![owner, expression], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            let expected_paths: Vec<String> = target
                .filter(|(path, _)| *path != owner)
                .map(|(path, _)| path.to_owned())
                .into_iter()
                .collect();
            if dependencies != expected_paths {
                failures.push(format!("{phase}: {owner} {expression} dependencies {dependencies:?}, expected {expected_paths:?}"));
            }
        }
    };
    let snapshot = |conn: &Connection| -> Vec<Vec<String>> {
        [
            "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
            "SELECT json_array(s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence) FROM edge_occurrences o JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id JOIN coverage_evidence e ON e.evidence_id=o.evidence_id JOIN coverage_evidence c ON c.evidence_id=o.confidence_id ORDER BY s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence",
            "SELECT json_array(p.path,c.line,x.expression,c.status,e.evidence) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id ORDER BY p.path,c.line,x.expression,c.status,e.evidence",
            "SELECT json_array(p.path,d.ordinal,f.path,d.kind,d.symbol,d.resolution) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id LEFT JOIN files f ON f.path_hash=d.target_hash ORDER BY p.path,d.ordinal",
            "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
        ].iter().map(|sql| conn.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()).collect()
    };
    let parity = |w: &Workspace, phase: &str, failures: &mut Vec<String>| {
        let incremental = w.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = w.0.join(".forge/rust-cold.sqlite");
        writer::build(&w.0, &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, &w.0).unwrap();
        commitments::verify(&cold).unwrap();
        if snapshot(&incremental) != snapshot(&cold) {
            failures.push(format!("{phase}: persisted cold/delta projections differ"));
        }
    };

    let w = Workspace::new();
    w.write("src/lib.rs", "pub mod provider;\nmod parent;\npub mod facade;\npub mod bridge;\nmod consumer;\nmod cycle_a;\nmod cycle_b;\nmod a;\nmod b;\npub mod another;\npub mod glob_one;\npub mod glob_two;\npub mod glob_ambiguous;\npub mod glob_same;\npub mod glob_mixed;\npub mod glob_outer;\npub mod glob_cycle;\npub mod glob_missing;\nmod glob_consumer;\nmod glob_shadow;\nmod glob_named;\nmod glob_broken;\nmod glob_negative;\nmod lexical_direct;\nmod lexical_typed;\nmod lexical_transitive;\nmod lexical_block;\nmod lexical_named;\nmod lexical_broken;\nmod lexical_own;\nmod lexical_private;\nmod lexical_ambiguous;\nmod lexical_incomplete;\nmod module_facade;\nmod module_outer;\nmod module_direct;\nmod module_forwarded;\n#[path = \"other.rs\"] mod attributed;\n#[cfg(any())] mod disabled;\nfn declared_probe() {\n    provider::build();\n    undeclared::build();\n    attributed::build();\n    disabled::build();\n}\n");
    let provider = "pub fn build() {}\npub fn other() {}\nfn hidden() {}\npub struct Workspace;\nimpl Workspace { pub fn new() -> Self { Self } pub fn work(&self) {} }\n";
    w.write("src/provider.rs", provider);
    for path in ["src/undeclared.rs", "src/attributed.rs", "src/disabled.rs"] {
        w.write(path, "pub fn build() {}\n");
    }
    let parent = "use crate::provider::{build as writer, Workspace};\nuse crate::bridge::twice as inherited;\nmod child;\n";
    w.write("src/parent/mod.rs", parent);
    w.write("src/parent/child.rs", "use super::{writer as emit, Workspace as Room};\nuse super::*;\nuse crate::provider::{self as direct};\nmod grandchild;\nfn probe() {\n    emit();\n    inherited();\n    Room::new();\n    direct::build();\n}\n");
    w.write(
        "src/parent/child/grandchild.rs",
        "use super::*;\nfn probe() {\n    inherited();\n    writer();\n}\n",
    );
    let glob_one = "pub use crate::provider::*;\n";
    for (path, source) in [
        ("src/lexical_direct.rs", "fn probe() { use crate::provider::*;\n build();\n }\n"),
        ("src/lexical_typed.rs", "fn probe() { use crate::glob_two::*;\n let value = Workspace::new();\n value.work();\n }\n"),
        ("src/lexical_transitive.rs", "fn probe() { use crate::glob_two::*;\n build();\n }\n"),
        ("src/lexical_block.rs", "fn probe() { { use crate::glob_two::*;\n build();\n } }\n"),
        ("src/lexical_named.rs", "fn probe() { use crate::glob_two::*; use crate::provider::other as build;\n build();\n }\n"),
        ("src/lexical_broken.rs", "fn probe() { use crate::glob_two::*; use crate::provider::missing as build;\n build();\n }\n"),
        ("src/lexical_own.rs", "fn probe() { use crate::glob_two::*; fn build() {}\n build();\n }\n"),
        ("src/lexical_private.rs", "fn probe() { use crate::provider::*;\n hidden();\n }\n"),
        ("src/lexical_ambiguous.rs", "fn probe() { use crate::provider::*; use crate::another::*;\n build();\n }\n"),
        ("src/lexical_incomplete.rs", "fn probe() { use crate::provider::*; use crate::glob_missing::*;\n build();\n }\n"),
        ("src/module_facade/mod.rs", "pub mod nested;\nmod private_nested;\n"),
        ("src/module_facade/nested.rs", "pub fn build() {}\n"),
        ("src/module_facade/private_nested.rs", "pub fn build() {}\n"),
        ("src/module_outer.rs", "pub use crate::module_facade::*;\n"),
        ("src/module_direct.rs", "use crate::module_facade::*;\nfn probe() {\n nested::build();\n private_nested::build();\n}\n"),
        ("src/module_forwarded.rs", "use crate::module_outer::*;\nfn probe() {\n nested::build();\n private_nested::build();\n}\n"),
        ("src/another.rs", "pub fn build() {} pub fn private_only() {}\n"),
        ("src/glob_one.rs", glob_one),
        ("src/glob_two.rs", "pub use crate::{glob_one::*, provider::{Workspace as Room}};\n"),
        ("src/glob_ambiguous.rs", "pub use crate::{provider::*, another::*};\n"),
        ("src/glob_same.rs", "pub use crate::{glob_one::*, glob_two::*};\n"),
        ("src/glob_mixed.rs", "pub use crate::provider::*; use crate::another::*;\n"),
        ("src/glob_outer.rs", "pub use crate::glob_mixed::*;\n"),
        ("src/glob_cycle.rs", "pub use crate::{provider::*, glob_cycle::*};\n"),
        ("src/glob_missing.rs", "pub use crate::{provider::*, missing::*};\n"),
        ("src/glob_consumer.rs", "use crate::glob_two::*;\nfn probe(value: Workspace) {\n    build();\n    Workspace::new();\n    value.work();\n    hidden();\n}\n"),
        ("src/glob_shadow.rs", "use crate::glob_two::*; fn build() {} fn probe() { build(); }\n"),
        ("src/glob_named.rs", "use crate::glob_two::*; use crate::provider::other as build; fn probe() { build(); }\n"),
        ("src/glob_broken.rs", "use crate::glob_two::*; use crate::provider::missing as build; fn probe() { build(); }\n"),
        ("src/glob_negative.rs", "use crate::glob_ambiguous::*;\nuse crate::glob_ambiguous as ambiguous;\nuse crate::glob_same as same;\nuse crate::glob_mixed as mixed;\nuse crate::glob_outer as outer;\nuse crate::glob_cycle as cyclic;\nuse crate::glob_missing as missing;\nfn probe() {\n    build();\n    ambiguous::build();\n    same::build();\n    mixed::build();\n    mixed::other();\n    mixed::private_only();\n    outer::build();\n    outer::other();\n    cyclic::build();\n    missing::build();\n}\n"),
    ] { w.write(path, source); }
    let facade = "pub use crate::provider::{build as forwarded, Workspace as Room};\npub use crate::provider as tools;\npub use crate::provider::missing as absent;\npub use crate::provider::build as conflict;\npub use crate::provider::other as conflict;\npub use crate::parent::writer as leaked;\npub use crate::provider::build as partial;\npub use crate::provider::missing as partial;\npub use crate::provider::build as cyclic;\npub use crate::cycle_a::ring as cyclic;\npub use crate::provider::build as same;\npub use crate::provider::build as same;\npub use crate::provider::build as hidden_mix;\npub use crate::parent::writer as hidden_mix;\npub use crate::provider::build as deep_mix;\npub use crate::bridge::depth0 as deep_mix;\n";
    w.write("src/facade.rs", facade);
    w.write(
        "src/bridge.rs",
        &format!("pub use crate::facade::{{forwarded as twice, Room as Hall, tools}};\n{}pub use crate::provider::build as depth64;\n", (0..64).map(|depth| format!("pub use self::depth{} as depth{depth};\n", depth + 1)).collect::<String>()),
    );
    w.write("src/consumer.rs", "use crate::bridge::{twice as invoke, Hall, tools};\nuse super::facade::{self as api, absent, conflict, leaked, partial, cyclic, same, hidden_mix, deep_mix};\nuse crate::cycle_a::ring;\nuse crate::parent::writer as private_parent;\nfn probe() {\n    invoke();\n    Hall::new();\n    tools::build();\n    api::forwarded();\n    absent();\n    conflict();\n    ring();\n    private_parent();\n    leaked();\n    partial();\n    api::partial();\n    cyclic();\n    api::cyclic();\n    same();\n    api::same();\n    hidden_mix();\n    api::hidden_mix();\n    deep_mix();\n    api::deep_mix();\n}\n");
    w.write("src/cycle_a.rs", "pub use crate::cycle_b::ring;\n");
    w.write("src/cycle_b.rs", "pub use crate::cycle_a::ring;\n");
    w.write("src/a.rs", "use crate::provider::build as hidden;\n");
    w.write(
        "src/b/mod.rs",
        "use crate::a::hidden as indirect;\nmod child;\n",
    );
    w.write(
        "src/b/child.rs",
        "use super::indirect;\nfn probe() {\n    indirect();\n}\n",
    );
    let probes = [
        (
            "src/lexical_typed.rs",
            "Workspace::new",
            Some(("src/provider.rs", "new")),
            "resolved",
        ),
        (
            "src/lexical_typed.rs",
            "value.work",
            Some(("src/provider.rs", "work")),
            "resolved",
        ),
        (
            "src/lexical_direct.rs",
            "build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/lexical_transitive.rs",
            "build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/lexical_block.rs",
            "build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/lexical_named.rs",
            "build",
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        ("src/lexical_broken.rs", "build", None, "unresolved"),
        (
            "src/lexical_own.rs",
            "build",
            Some(("src/lexical_own.rs", "build")),
            "resolved",
        ),
        ("src/lexical_private.rs", "hidden", None, "unresolved"),
        ("src/lexical_ambiguous.rs", "build", None, "ambiguous"),
        ("src/lexical_incomplete.rs", "build", None, "unresolved"),
        (
            "src/module_direct.rs",
            "nested::build",
            Some(("src/module_facade/nested.rs", "build")),
            "resolved",
        ),
        (
            "src/module_forwarded.rs",
            "nested::build",
            Some(("src/module_facade/nested.rs", "build")),
            "resolved",
        ),
        (
            "src/module_direct.rs",
            "private_nested::build",
            None,
            "unresolved",
        ),
        (
            "src/module_forwarded.rs",
            "private_nested::build",
            None,
            "unresolved",
        ),
        (
            "src/parent/child/grandchild.rs",
            "inherited",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/parent/child/grandchild.rs",
            "writer",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/glob_consumer.rs",
            "build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/glob_consumer.rs",
            "Workspace::new",
            Some(("src/provider.rs", "new")),
            "resolved",
        ),
        (
            "src/glob_consumer.rs",
            "value.work",
            Some(("src/provider.rs", "work")),
            "resolved",
        ),
        ("src/glob_consumer.rs", "hidden", None, "unresolved"),
        (
            "src/glob_shadow.rs",
            "build",
            Some(("src/glob_shadow.rs", "build")),
            "resolved",
        ),
        (
            "src/glob_named.rs",
            "build",
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        ("src/glob_broken.rs", "build", None, "unresolved"),
        ("src/glob_negative.rs", "build", None, "ambiguous"),
        (
            "src/glob_negative.rs",
            "ambiguous::build",
            None,
            "unresolved",
        ),
        (
            "src/glob_negative.rs",
            "same::build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        ("src/glob_negative.rs", "mixed::build", None, "unresolved"),
        (
            "src/glob_negative.rs",
            "mixed::other",
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        (
            "src/glob_negative.rs",
            "mixed::private_only",
            None,
            "unresolved",
        ),
        ("src/glob_negative.rs", "outer::build", None, "unresolved"),
        (
            "src/glob_negative.rs",
            "outer::other",
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        ("src/glob_negative.rs", "cyclic::build", None, "unresolved"),
        ("src/glob_negative.rs", "missing::build", None, "unresolved"),
        (
            "src/lib.rs",
            "provider::build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        ("src/lib.rs", "undeclared::build", None, "unresolved"),
        ("src/lib.rs", "attributed::build", None, "unresolved"),
        ("src/lib.rs", "disabled::build", None, "unresolved"),
        (
            "src/parent/child.rs",
            "inherited",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/parent/child.rs",
            "emit",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/parent/child.rs",
            "Room::new",
            Some(("src/provider.rs", "new")),
            "resolved",
        ),
        (
            "src/parent/child.rs",
            "direct::build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/consumer.rs",
            "invoke",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/consumer.rs",
            "Hall::new",
            Some(("src/provider.rs", "new")),
            "resolved",
        ),
        (
            "src/consumer.rs",
            "tools::build",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/consumer.rs",
            "api::forwarded",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        ("src/consumer.rs", "absent", None, "unresolved"),
        ("src/consumer.rs", "conflict", None, "ambiguous"),
        ("src/consumer.rs", "ring", None, "unresolved"),
        ("src/consumer.rs", "private_parent", None, "unresolved"),
        ("src/consumer.rs", "leaked", None, "unresolved"),
        ("src/b/child.rs", "indirect", None, "unresolved"),
        ("src/consumer.rs", "partial", None, "unresolved"),
        ("src/consumer.rs", "api::partial", None, "unresolved"),
        ("src/consumer.rs", "cyclic", None, "unresolved"),
        ("src/consumer.rs", "api::cyclic", None, "unresolved"),
        ("src/consumer.rs", "hidden_mix", None, "unresolved"),
        ("src/consumer.rs", "api::hidden_mix", None, "unresolved"),
        ("src/consumer.rs", "deep_mix", None, "unresolved"),
        ("src/consumer.rs", "api::deep_mix", None, "unresolved"),
        (
            "src/consumer.rs",
            "same",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "src/consumer.rs",
            "api::same",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ];
    drop(w.build());
    observe(&w.open(), "cold forwarding", &probes, &mut failures);
    parity(&w, "cold forwarding", &mut failures);
    let frozen_consumer = fs::read(w.0.join("src/consumer.rs")).unwrap();
    let frozen_glob_consumer = fs::read(w.0.join("src/glob_consumer.rs")).unwrap();
    for (phase, source, target, status) in [
        (
            "retarget glob",
            "pub use crate::another::*;\n",
            Some(("src/another.rs", "build")),
            "resolved",
        ),
        ("remove glob", "", None, "unresolved"),
        (
            "restore glob",
            glob_one,
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/glob_one.rs", source);
        w.delta("src/glob_one.rs");
        observe(
            &w.open(),
            phase,
            &[("src/glob_consumer.rs", "build", target, status)],
            &mut failures,
        );
        assert_eq!(
            fs::read(w.0.join("src/glob_consumer.rs")).unwrap(),
            frozen_glob_consumer,
            "provider-only glob mutation"
        );
        parity(&w, phase, &mut failures);
    }
    for (phase, from, to, expression, target, status) in [
        (
            "missing becomes same canonical target",
            "missing as partial",
            "build as partial",
            "partial",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "missing becomes a distinct target",
            "missing as partial",
            "other as partial",
            "partial",
            None,
            "ambiguous",
        ),
        (
            "cycle removed",
            "pub use crate::cycle_a::ring as cyclic;",
            "",
            "cyclic",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "inaccessible alternative removed",
            "pub use crate::parent::writer as hidden_mix;",
            "",
            "hidden_mix",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "exhausted path shortened",
            "pub use crate::bridge::depth0 as deep_mix;",
            "pub use crate::bridge::depth64 as deep_mix;",
            "deep_mix",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/facade.rs", &facade.replace(from, to));
        w.delta("src/facade.rs");
        let member = format!("api::{expression}");
        observe(
            &w.open(),
            phase,
            &[
                ("src/consumer.rs", expression, target, status),
                ("src/consumer.rs", &member, target, status),
            ],
            &mut failures,
        );
        assert_eq!(
            fs::read(w.0.join("src/consumer.rs")).unwrap(),
            frozen_consumer,
            "provider-only forwarding mutation"
        );
        parity(&w, phase, &mut failures);
        w.write("src/facade.rs", facade);
        w.delta("src/facade.rs");
        observe(
            &w.open(),
            "restore incomplete alternatives",
            &probes,
            &mut failures,
        );
        parity(&w, "restore incomplete alternatives", &mut failures);
    }
    for (phase, replacement, target, status) in [
        (
            "retarget export",
            facade.replacen("build as forwarded", "other as forwarded", 1),
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        (
            "remove export",
            facade.replacen("build as forwarded, ", "", 1),
            None,
            "unresolved",
        ),
        (
            "restore export",
            facade.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/facade.rs", &replacement);
        w.delta("src/facade.rs");
        observe(
            &w.open(),
            phase,
            &[
                ("src/consumer.rs", "invoke", target, status),
                ("src/consumer.rs", "api::forwarded", target, status),
                ("src/parent/child.rs", "inherited", target, status),
            ],
            &mut failures,
        );
        parity(&w, phase, &mut failures);
    }
    for (phase, replacement, target, status) in [
        (
            "rename parent alias",
            parent.replace("as writer", "as renamed"),
            None,
            "unresolved",
        ),
        (
            "restore parent alias",
            parent.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/parent/mod.rs", &replacement);
        w.delta("src/parent/mod.rs");
        observe(
            &w.open(),
            phase,
            &[("src/parent/child.rs", "emit", target, status)],
            &mut failures,
        );
        parity(&w, phase, &mut failures);
    }
    for (phase, replacement, target, status) in [
        (
            "remove provider",
            provider.replace("pub fn build() {}\n", ""),
            None,
            "unresolved",
        ),
        (
            "restore provider",
            provider.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/provider.rs", &replacement);
        w.delta("src/provider.rs");
        observe(
            &w.open(),
            phase,
            &[
                ("src/consumer.rs", "invoke", target, status),
                ("src/parent/child.rs", "inherited", target, status),
            ],
            &mut failures,
        );
        parity(&w, phase, &mut failures);
    }

    for (case, manifest, library, expected) in [
        ("package name", "[package]\nname='root-kit'\n", true, true),
        (
            "explicit library name",
            "[package]\nname='different'\n[lib]\nname='root_kit'\n",
            true,
            true,
        ),
        (
            "wrong package",
            "[package]\nname='other-kit'\n",
            true,
            false,
        ),
        (
            "absent library",
            "[package]\nname='root-kit'\n",
            false,
            false,
        ),
        (
            "disabled library",
            "[package]\nname='root-kit'\nautolib=false\n",
            true,
            false,
        ),
        (
            "missing package name",
            "[workspace]\n[lib]\nname='root_kit'\n",
            true,
            false,
        ),
        (
            "inherited package name",
            "[package]\nname.workspace=true\n[lib]\nname='root_kit'\n",
            true,
            false,
        ),
        (
            "explicit enabled library",
            "[package]\nname='root-kit'\nautolib=false\n[lib]\n",
            true,
            true,
        ),
        (
            "unsupported custom path",
            "[package]\nname='root-kit'\n[lib]\npath='custom/lib.rs'\n",
            true,
            false,
        ),
    ] {
        let w = Workspace::new();
        w.write("Cargo.toml", manifest);
        if library {
            w.write("src/lib.rs", "pub fn build() {}\npub struct Workspace;\nimpl Workspace { pub fn new() -> Self { Self } }\n");
        }
        w.write("foreign/Cargo.toml", "[package]\nname='root-kit'\n");
        w.write("foreign/src/lib.rs", "pub fn build() {}\n");
        w.write("tests/api.rs", "use root_kit::{build as execute, Workspace};\nfn probe() {\n    execute();\n    Workspace::new();\n}\n");
        drop(w.build());
        let exact = [
            (
                "tests/api.rs",
                "execute",
                Some(("src/lib.rs", "build")),
                "resolved",
            ),
            (
                "tests/api.rs",
                "Workspace::new",
                Some(("src/lib.rs", "new")),
                "resolved",
            ),
        ];
        if expected {
            observe(&w.open(), case, &exact, &mut failures);
            w.write("Cargo.toml", "[package]\nname='renamed-kit'\n");
            w.delta("Cargo.toml");
        } else if case == "missing package name" || case == "inherited package name" {
            observe(
                &w.open(),
                case,
                &[
                    ("tests/api.rs", "execute", None, "external"),
                    ("tests/api.rs", "Workspace::new", None, "external"),
                ],
                &mut failures,
            );
        }
        let calls: i64 = w.open().query_row("SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path='tests/api.rs' AND o.kind='calls'", [], |r| r.get(0)).unwrap();
        if calls != 0 {
            failures.push(format!(
                "{case}: absent or renamed root identity emitted {calls} exact calls"
            ));
        }
        parity(&w, case, &mut failures);
        if expected {
            w.write("Cargo.toml", manifest);
            w.delta("Cargo.toml");
            observe(&w.open(), "restored root identity", &exact, &mut failures);
            parity(&w, "restored root identity", &mut failures);
        }
    }
    let w = Workspace::new();
    w.write("Cargo.toml", "[package]\nname='root-kit'\n");
    w.write("src/lib.rs", "pub mod provider;\n");
    w.write("src/provider.rs", "pub fn build() {}\n");
    w.write("foreign/Cargo.toml", "[package]\nname='foreign-kit'\n");
    w.write("foreign/src/lib.rs", "pub mod provider;\nmod consumer;\n");
    w.write("foreign/src/provider.rs", "pub fn build() {}\n");
    for path in ["src/consumer.rs", "foreign/src/consumer.rs"] {
        w.write(path, "use crate::provider::build as from_crate;\nuse super::provider::build as from_super;\nuse self::local as own_alias;\nuse missing_api::build as external;\nfn local() {}\nfn probe() {\n    from_crate();\n    from_super();\n    own_alias();\n    external();\n}\n");
    }
    w.write(
        "foreign/tests/probe.rs",
        "use root_kit::provider::build as execute;\nfn probe() { execute(); }\n",
    );
    w.write(
        "tests/control.rs",
        "use root_kit::provider::build as execute;\nfn probe() { execute(); }\n",
    );
    drop(w.build());
    observe(
        &w.open(),
        "nested consumer package boundary",
        &[
            (
                "tests/control.rs",
                "execute",
                Some(("src/provider.rs", "build")),
                "resolved",
            ),
            ("foreign/tests/probe.rs", "execute", None, "unresolved"),
        ],
        &mut failures,
    );
    parity(&w, "nested consumer package boundary", &mut failures);
    for (phase, manifest) in [
        ("valid root metadata", "[package]\nname='root-kit'\n"),
        ("missing root metadata", "[workspace]\n"),
        (
            "inherited root metadata",
            "[package]\nname.workspace=true\n",
        ),
        ("restored root metadata", "[package]\nname='root-kit'\n"),
    ] {
        w.write("Cargo.toml", manifest);
        w.delta("Cargo.toml");
        for (owner, provider) in [
            ("src/consumer.rs", "src/provider.rs"),
            ("foreign/src/consumer.rs", "foreign/src/provider.rs"),
        ] {
            observe(
                &w.open(),
                phase,
                &[
                    (owner, "from_crate", Some((provider, "build")), "resolved"),
                    (owner, "from_super", Some((provider, "build")), "resolved"),
                    (owner, "own_alias", Some((owner, "local")), "resolved"),
                    (owner, "external", None, "external"),
                ],
                &mut failures,
            );
        }
        parity(&w, phase, &mut failures);
    }

    assert!(
        failures.is_empty(),
        "Rust binding contract failures:\n{}",
        failures.join("\n")
    );
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
        let coverage: Vec<(i64, String, String, String)> = conn
            .prepare("SELECT c.line, e.expression, c.status, v.evidence FROM resolution_coverage c JOIN coverage_expressions e ON e.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id ORDER BY c.line, e.expression, c.status, v.evidence")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let resolved =
            "1 lexically justified candidates; implementor resolved=true, heuristic=false";
        let shadowed =
            "callee is shadowed by a parameter or local binding of unknown callable identity";
        let dynamic = "computed receiver or dynamic callee; callable identity is unknown";
        let expected_coverage = match language {
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
        };
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
            expected_coverage,
            "{language} coverage classifications"
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
