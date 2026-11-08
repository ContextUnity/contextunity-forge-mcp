use crate::languages::harness::ExtractorHarness;
use contextunity_forge_mcp::{
    core::{
        models::Facts,
        semantic::{BindingFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
    },
    engine::{
        ast::ScopeBindings,
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
        let mut facts = ValueFlowFacts {
            return_type: self.flow_return.then(|| named("FlowReturn")),
            bindings: vec![BindingFact {
                name: "flow_local".into(),
                position: SourcePosition { line: 4, column: 2 },
                value: ValueExpr::Alias {
                    name: "input".into(),
                },
                conditional: false,
            }],
            ..Default::default()
        };
        facts.alias_type = Some(named("ContextAlias"));
        facts.return_position = Some(SourcePosition { line: 5, column: 2 });
        facts
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

type Case = (bool, bool, Prepared);

fn cases() -> impl Iterator<Item = Case> {
    [false, true].into_iter().flat_map(|combined| {
        [false, true].into_iter().flat_map(move |flow_return| {
            [Prepared::Absent, Prepared::Matching, Prepared::Mismatched]
                .into_iter()
                .map(move |prepared| (combined, flow_return, prepared))
        })
    })
}

fn extract((combined, flow_return, prepared): Case) -> (Facts, Vec<&'static str>) {
    let default = DefaultHooks::new(prepared, flow_return);
    let combined_profile = CombinedHooks(DefaultHooks::new(prepared, flow_return));
    let (profile, trace): (&dyn LanguageProfile, _) = if combined {
        (&combined_profile, &combined_profile.0.calls)
    } else {
        (&default, &default.calls)
    };
    let extracted = ExtractorHarness::for_lang("javascript")
        .at_path(PATH)
        .extract("");
    let mut facts: Facts =
        serde_json::from_value(serde_json::to_value(extracted).expect("public facts serialize"))
            .expect("public facts round-trip");
    let module = facts.nodes.iter_mut().find(|n| n.kind == "module").unwrap();
    module.details["bindings"] = json!(["existing_binding"]);
    module.details["rebindings"] = json!(["existing_rebound"]);
    languages::parse_file(profile, PATH, SOURCE, "legacy", &mut facts).unwrap();
    let calls = trace.lock().unwrap().clone();
    (facts, calls)
}

#[test]
fn legacy_and_combined_declaration_hooks_preserve_public_metadata() {
    for case in cases() {
        let (facts, _) = extract(case);
        assert!(facts.errors.is_empty(), "{case:?}: {:?}", facts.errors);
        let symbol = facts.nodes.iter().find(|n| n.name == "sample").unwrap();
        assert_eq!(symbol.qualname, "legacy.ReceiverType.sample", "{case:?}");
        let details = crate::common::assertions::projected_node_details(&facts, symbol);
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
            assert_eq!(details[key], expected, "{case:?}: {key}");
        }
    }
}

#[test]
fn contextual_flow_keeps_precedence_bindings_and_source_positions() {
    for case in cases() {
        let (facts, _) = extract(case);
        let symbol = facts.nodes.iter().find(|n| n.name == "sample").unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, symbol);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
        assert_eq!(flow.alias_type, Some(named("ContextAlias")), "{case:?}");
        assert_eq!(
            flow.return_type,
            Some(named(if case.1 {
                "FlowReturn"
            } else {
                "MetadataReturn"
            })),
            "{case:?}"
        );
        assert_eq!(flow.bindings.len(), 1, "{case:?}");
        assert_eq!(flow.bindings[0].name, "flow_local", "{case:?}");
        assert_eq!(flow.bindings[0].position.line, 4, "{case:?}");
        assert_eq!(
            flow.return_position,
            Some(SourcePosition { line: 5, column: 2 }),
            "{case:?}"
        );
    }
}

#[test]
fn exact_prepared_root_bindings_merge_with_existing_public_bindings() {
    for case in cases() {
        let (facts, _) = extract(case);
        let module = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, module);
        let matching = matches!(case.2, Prepared::Matching);
        assert_eq!(
            details["bindings"],
            if matching {
                json!(["existing_binding", "prepared_only"])
            } else {
                json!(["existing_binding", "fallback"])
            },
            "{case:?}"
        );
        assert_eq!(
            details["rebindings"],
            if matching {
                json!(["existing_rebound", "prepared_rebound"])
            } else {
                json!(["existing_rebound", "fallback"])
            },
            "{case:?}"
        );
    }
}

#[test]
fn declaration_hooks_and_root_fallback_execute_exactly_once() {
    for case in cases() {
        let (_, calls) = extract(case);
        let matching = matches!(case.2, Prepared::Matching);
        assert_eq!(
            calls.iter().filter(|c| **c == "root_fallback").count(),
            usize::from(!matching),
            "{case:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| **c == "combined_declaration")
                .count(),
            usize::from(case.0),
            "{case:?}"
        );
        assert!(!calls.contains(&"separate_metadata"), "{case:?}");
        assert_eq!(
            calls.iter().filter(|c| **c == "metadata").count(),
            usize::from(!case.0),
            "{case:?}"
        );
    }
}
