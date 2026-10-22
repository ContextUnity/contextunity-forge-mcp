use contextunity_forge_mcp::{
    core::{
        models::Facts,
        semantic::{BindingFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
    },
    engine::{
        ast::{self, ScopeBindings},
        languages::{self, FileContext, ImportPath, LanguageProfile, SymbolMetadata},
    },
};
use serde_json::json;
use std::sync::Mutex;

const SOURCE: &str = "let fallback = 1;\nfallback = 2;\nconst assigned = function sample(input) {\n  const local = input;\n  return local;\n};\n";
const PATH: &str = "legacy.js";

#[derive(Clone, Copy, Debug)]
enum Prepared {
    Absent,
    Matching,
    Mismatched,
}

struct DefaultHooks {
    prepared: Prepared,
    flow_return: bool,
    calls: Mutex<Vec<&'static str>>,
}
impl DefaultHooks {
    fn new(prepared: Prepared, flow_return: bool) -> Self {
        Self {
            prepared,
            flow_return,
            calls: Mutex::new(Vec::new()),
        }
    }
    fn metadata_value(&self) -> SymbolMetadata {
        SymbolMetadata {
            return_type: Some(named("MetadataReturn")),
            param_types: [("input".into(), "InputType".into())].into(),
            receiver_name: Some("receiver_name".into()),
            receiver_type: Some("ReceiverType".into()),
            receiver: Some("receiver".into()),
            object_ownership: None,
            decorators: vec!["decorate".into()],
            bases: Some("Base".into()),
            is_method: Some(true),
            is_static: Some(true),
            default_export: true,
            is_async: true,
            is_overload: true,
            is_stub: true,
            is_conditional: true,
            is_public: None,
        }
    }
    fn contextual_flow(&self, node: tree_sitter::Node<'_>) -> ValueFlowFacts {
        self.calls.lock().unwrap().push("contextual_flow");
        if node.kind() != "function_expression" {
            return ValueFlowFacts::default();
        }
        ValueFlowFacts {
            alias_type: Some(named("ContextAlias")),
            return_type: self.flow_return.then(|| named("FlowReturn")),
            return_position: Some(SourcePosition { line: 5, column: 2 }),
            bindings: vec![BindingFact {
                name: "flow_local".into(),
                position: SourcePosition { line: 4, column: 2 },
                value: ValueExpr::Alias {
                    name: "input".into(),
                },
                conditional: false,
            }],
            ..Default::default()
        }
    }
}

fn named(name: &str) -> TypeExpr {
    TypeExpr::Named { name: name.into() }
}

impl LanguageProfile for DefaultHooks {
    fn id(&self) -> &'static str {
        "javascript"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["js"]
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        languages::require("javascript").unwrap().grammar(path)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        (kind == "function_expression").then_some("function")
    }
    fn node_prefix(&self, _: &str) -> &'static str {
        "legacy"
    }
    fn normalize_import(&self, _: &str, _: &str) -> Option<ImportPath> {
        None
    }
    fn prepare(&self, root: tree_sitter::Node<'_>, _: &str) -> FileContext {
        let root_bindings = match self.prepared {
            Prepared::Absent => None,
            Prepared::Matching | Prepared::Mismatched => Some((
                if matches!(self.prepared, Prepared::Matching) {
                    root.id()
                } else {
                    0
                },
                ScopeBindings {
                    all: vec!["prepared_only".into()],
                    rebindings: vec!["prepared_rebound".into()],
                },
            )),
        };
        FileContext {
            root_bindings,
            ..Default::default()
        }
    }
    fn bindings(&self, node: tree_sitter::Node<'_>, source: &str) -> ScopeBindings {
        if node.parent().is_none() {
            self.calls.lock().unwrap().push("root_fallback");
        }
        languages::require("javascript")
            .unwrap()
            .bindings(node, source)
    }
    fn metadata(
        &self,
        _: tree_sitter::Node<'_>,
        _: &str,
        _: &str,
        _: &FileContext,
    ) -> SymbolMetadata {
        self.calls.lock().unwrap().push("metadata");
        self.metadata_value()
    }
    fn value_flow(&self, _: tree_sitter::Node<'_>, _: &str) -> ValueFlowFacts {
        panic!("default declaration must call the contextual flow hook")
    }
    fn value_flow_with_context(
        &self,
        node: tree_sitter::Node<'_>,
        _: &str,
        _: &FileContext,
    ) -> ValueFlowFacts {
        self.contextual_flow(node)
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

struct CombinedHooks(DefaultHooks);
impl LanguageProfile for CombinedHooks {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn extensions(&self) -> &'static [&'static str] {
        self.0.extensions()
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        self.0.grammar(path)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        self.0.symbol_kind(kind)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        self.0.node_prefix(kind)
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        self.0.normalize_import(owner, module)
    }
    fn prepare(&self, root: tree_sitter::Node<'_>, source: &str) -> FileContext {
        self.0.prepare(root, source)
    }
    fn bindings(&self, node: tree_sitter::Node<'_>, source: &str) -> ScopeBindings {
        self.0.bindings(node, source)
    }
    fn metadata(
        &self,
        _: tree_sitter::Node<'_>,
        _: &str,
        _: &str,
        _: &FileContext,
    ) -> SymbolMetadata {
        self.0.calls.lock().unwrap().push("separate_metadata");
        SymbolMetadata::default()
    }
    fn declaration(
        &self,
        node: tree_sitter::Node<'_>,
        _: &str,
        _: &str,
        _: &FileContext,
    ) -> (SymbolMetadata, ValueFlowFacts) {
        self.0.calls.lock().unwrap().push("combined_declaration");
        (self.0.metadata_value(), self.0.contextual_flow(node))
    }
    fn value_flow_with_context(
        &self,
        node: tree_sitter::Node<'_>,
        _: &str,
        _: &FileContext,
    ) -> ValueFlowFacts {
        self.0.contextual_flow(node)
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
fn legacy_declaration_and_exact_root_preparation_preserve_public_facts() {
    let mut failures = Vec::new();
    for combined in [false, true] {
        for flow_return in [false, true] {
            for prepared in [Prepared::Absent, Prepared::Matching, Prepared::Mismatched] {
                let default = DefaultHooks::new(prepared, flow_return);
                let combined_profile = CombinedHooks(DefaultHooks::new(prepared, flow_return));
                let (profile, trace): (&dyn LanguageProfile, _) = if combined {
                    (&combined_profile, &combined_profile.0.calls)
                } else {
                    (&default, &default.calls)
                };
                let label =
                    format!("combined={combined}/flow_return={flow_return}/prepared={prepared:?}");
                let mut facts = ast::extract(PATH, "javascript", "").unwrap();
                let module = facts.nodes.iter_mut().find(|n| n.kind == "module").unwrap();
                module.details["bindings"] = json!(["existing_binding"]);
                module.details["rebindings"] = json!(["existing_rebound"]);
                languages::parse_file(profile, PATH, SOURCE, "legacy", &mut facts).unwrap();
                let mut check = |ok: bool, message: &str| {
                    if !ok {
                        failures.push(format!("{label}: {message}"));
                    }
                };
                check(facts.errors.is_empty(), "real grammar parse diagnostics");
                let symbol = facts.nodes.iter().find(|n| n.name == "sample").unwrap();
                check(
                    symbol.qualname == "legacy.ReceiverType.sample",
                    "metadata receiver qualification",
                );
                let details = &symbol.details;
                for (key, expected) in [
                    ("receiver_name", json!("receiver_name")),
                    ("receiver_type", json!("ReceiverType")),
                    ("receiver", json!("receiver")),
                    ("decorators", json!(["decorate"])),
                    ("bases", json!("Base")),
                    ("param_types", json!({"input":"InputType"})),
                    ("default_export", json!(true)),
                    ("async", json!(true)),
                    ("prototype", json!(true)),
                    ("is_method", json!(true)),
                    ("is_static", json!(true)),
                    ("is_overload", json!(true)),
                    ("is_stub", json!(true)),
                    ("is_conditional", json!(true)),
                ] {
                    check(details[key] == expected, &format!("metadata field {key}"));
                }
                let flow: ValueFlowFacts =
                    serde_json::from_value(details["value_flow"].clone()).unwrap();
                check(
                    flow.alias_type == Some(named("ContextAlias")),
                    "contextual flow preserved with metadata",
                );
                check(
                    flow.return_type
                        == Some(named(if flow_return {
                            "FlowReturn"
                        } else {
                            "MetadataReturn"
                        })),
                    "flow precedence / metadata return fallback",
                );
                check(
                    flow.bindings.len() == 1
                        && flow.bindings[0].name == "flow_local"
                        && flow.bindings[0].position.line == 4,
                    "serialized typed binding and source position",
                );
                check(
                    flow.return_position == Some(SourcePosition { line: 5, column: 2 }),
                    "serialized return source position",
                );
                let module = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
                let matching = matches!(prepared, Prepared::Matching);
                check(
                    module.details["bindings"]
                        == if matching {
                            json!(["existing_binding", "prepared_only"])
                        } else {
                            json!(["existing_binding", "fallback"])
                        },
                    "matching prepared bindings / canonical fallback merge",
                );
                check(
                    module.details["rebindings"]
                        == if matching {
                            json!(["existing_rebound", "prepared_rebound"])
                        } else {
                            json!(["existing_rebound", "fallback"])
                        },
                    "matching prepared rebindings / canonical fallback merge",
                );
                let calls = trace.lock().unwrap();
                check(
                    calls.iter().filter(|c| **c == "root_fallback").count()
                        == usize::from(!matching),
                    "prepared guard chooses canonical root fallback exactly",
                );
                check(
                    calls
                        .iter()
                        .filter(|c| **c == "combined_declaration")
                        .count()
                        == usize::from(combined),
                    "combined declaration called once",
                );
                check(
                    !calls.contains(&"separate_metadata"),
                    "combined metadata must not be replaced by separate hook",
                );
                check(
                    calls.iter().filter(|c| **c == "metadata").count() == usize::from(!combined),
                    "default metadata called once",
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
