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

#[test]
fn syntax_grammar_cache_isolated_by_spec_and_actual_grammar() {
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
