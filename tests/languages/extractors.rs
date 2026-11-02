#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::{ast, languages::rust, linker};
#[cfg(feature = "lang-rust")]
use forge_index::languages::rust::kind;
#[cfg(all(feature = "lang-rust", not(test)))]
use forge_index::linker::rust::resolution::is_registered_cargo_crate;

#[cfg(feature = "lang-rust")]
#[cfg(test)]
fn is_registered_cargo_crate(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "rusqlite"
            | "tree-sitter"
            | "tree_sitter"
            | "serde"
            | "serde_json"
            | "tokio"
            | "anyhow"
            | "clap"
            | "tracing"
            | "rayon"
            | "tempfile"
            | "regex"
            | "thiserror"
            | "parking_lot"
            | "crossbeam"
            | "log"
    )
}
#[cfg(feature = "lang-rust")]
use forge_index::linker::value_flow::ValueFlowIndex;

#[cfg(feature = "lang-rust")]
struct SeamValueFlowResolver;

#[cfg(feature = "lang-rust")]
impl<'a> forge_index::linker::value_flow::SemanticResolver<'a> for SeamValueFlowResolver {
    fn resolve_symbol(
        &self,
        _scope: &'a contextunity_forge_mcp::core::models::Node,
        _name: &str,
        _at: contextunity_forge_mcp::core::semantic::SourcePosition,
        _role: forge_index::linker::value_flow::SymbolRole,
    ) -> forge_index::linker::value_flow::Symbol<'a> {
        forge_index::linker::value_flow::Symbol::Unknown
    }
}

#[cfg(feature = "lang-python")]
mod scope_allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Probe;

    thread_local! {
        static SELECTED: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    #[global_allocator]
    static ALLOCATOR: Probe = Probe;

    fn record(size: usize, align: usize, pointer: *mut u8) {
        if align == 1 && !pointer.is_null() {
            let _ = SELECTED.try_with(|selected| {
                let (expected, count) = selected.get();
                if expected != 0 && size == expected {
                    selected.set((expected, count + 1));
                }
            });
        }
    }

    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            record(size, layout.align(), pointer);
            pointer
        }
    }

    pub(super) fn measure<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                SELECTED.with(|selected| selected.set((0, 0)));
            }
        }
        SELECTED.with(|selected| {
            assert_eq!(selected.get().0, 0);
            selected.set((size, 0));
        });
        let reset = Reset;
        let result = operation();
        let count = SELECTED.with(|selected| selected.get().1);
        drop(reset);
        (result, count)
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_ast_extractor() {
    {
        let member_rules = |returns: Option<&str>| {
            let mut manifest = String::from("enabled = true\nreplace = true\n");
            for package in ["serde_json", "serde_yaml"] {
                manifest.push_str(&format!("[[class_hierarchy_receivers]]\nbases = ['{package}']\nprovider = '{package}'\nmembers = ['from_str', 'from_slice', 'from_reader', 'from_value']\n"));
                if let Some(base) = returns {
                    manifest.push_str("[class_hierarchy_receivers.returns]\n");
                    for member in ["from_str", "from_slice", "from_reader", "from_value"] {
                        manifest.push_str(&format!("{member} = {{ kind = 'receiver', base = '{base}' }}\n"));
                    }
                }
            }
            manifest
        };
        for (policy, manifest, inferred) in [
            ("enabled", None, true),
            ("disabled", Some("enabled = false\n".to_owned()), false),
            ("replace-empty", Some("enabled = true\nreplace = true\n".to_owned()), false),
            ("replace-no-returns", Some(member_rules(None)), false),
            ("replace-result", Some(member_rules(Some("Result"))), true),
            ("replace-non-result", Some(member_rules(Some("String"))), false),
        ] {
            let workspace = super::support::Workspace::new();
            workspace.write("Cargo.toml", "[package]\nname='batch11-probe'\nversion='0.1.0'\n[dependencies]\nserde_json='1'\nserde_yaml='0.9'\nserde={version='1',features=['derive']}\n");
            if let Some(manifest) = manifest { workspace.write(".forge/frameworks/rust-cargo.toml", &manifest); }
            let mut source = String::from("#[derive(serde::Deserialize)] struct LeafBatch11 {} impl LeafBatch11 { fn finish(&self) {} }\n");
            let mut functions = Vec::new();
            for (package, alias, methods) in [
                ("serde_json", "json", &["from_str", "from_slice", "from_reader", "from_value"][..]),
                ("serde_yaml", "yaml", &["from_str", "from_slice", "from_reader"][..]),
            ] {
                source.push_str(&format!("use {package} as {alias};\n"));
                for method in methods {
                    source.push_str(&format!("use {package}::{method} as {alias}_{method};\n"));
                    let reader = if *method == "from_reader" { "_, " } else { "" };
                    let input = match *method {
                        "from_slice" => "b\"{}\"",
                        "from_reader" => "\"{}\".as_bytes()",
                        "from_value" => "serde_json::Value::Null",
                        _ => "\"{}\"",
                    };
                    for (spelling, callee) in [
                        ("direct", format!("{package}::{method}")),
                        ("namespace", format!("{alias}::{method}")),
                        ("function", format!("{alias}_{method}")),
                    ] {
                        let function = format!("probe_{alias}_{method}_{spelling}");
                        source.push_str(&format!("fn {function}() {{ {callee}::<{reader}LeafBatch11>({input}).unwrap().finish(); }}\n"));
                        source.push_str(&format!("fn {function}_assigned() {{ let parsed = {callee}::<{reader}LeafBatch11>({input}).expect(\"valid\"); parsed.finish(); }}\n"));
                        source.push_str(&format!("fn {function}_annotated() {{ let parsed: Result<LeafBatch11, {package}::Error> = {callee}({input}); parsed.unwrap().finish(); }}\n"));
                        functions.push(function);
                    }
                }
            }
            workspace.write("src/lib.rs", &source);
            let db = workspace.build();
            for function in functions {
                for (suffix, expected) in [("", inferred), ("_assigned", inferred), ("_annotated", true)] {
                    let function = format!("{function}{suffix}");
                    let edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname LIKE '%LeafBatch11.finish' AND e.kind='calls'", [&function], |row| row.get(0)).unwrap();
                    assert_eq!(edges, i64::from(expected), "{policy}: {function}: effective signature authority");
                    if !expected {
                        let resolved: i64 = db.query_row("SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN nodes n ON n.path_id=c.path_id AND c.line BETWEEN n.line AND n.end_line WHERE n.name=?1 AND x.expression LIKE '%finish' AND c.status!='unresolved'", [&function], |row| row.get(0)).unwrap();
                        assert_eq!(resolved, 0, "{policy}: {function}: unknown inferred payload must fail closed");
                    }
                }
            }
        }
    }
    {
        let workspace = super::support::Workspace::new();
        workspace.write("Cargo.toml", "[package]\nname='batch10-probe'\nversion='0.1.0'\n[dependencies]\nserde_json='1'\nserde_yaml='0.9'\nserde={version='1',features=['derive']}\n");
        let mut source = String::from(r#"
#[derive(serde::Deserialize)] struct LeafBatch10 {}
impl LeafBatch10 { fn finish(&self) {} }
struct HubBatch10 {}
impl HubBatch10 { fn leaf(&self) -> LeafBatch10 { LeafBatch10 {} } }
struct ChangingBatch10 {}
impl ChangingBatch10 {
    fn unwrap(self) -> LeafBatch10 { LeafBatch10 {} }
    fn expect(self, _: &str) -> LeafBatch10 { LeafBatch10 {} }
}
fn missing_unwrap(h: HubBatch10) { h.unwrap().leaf(); }
fn missing_expect(h: HubBatch10) { h.expect("missing").leaf(); }
fn missing_try(h: HubBatch10) -> Result<(), ()> { h?.leaf(); Ok(()) }
fn assigned_missing(h: HubBatch10) { let changed = h.unwrap(); changed.leaf(); }
fn declared_unwrap(h: ChangingBatch10) { h.unwrap().finish(); }
fn declared_expect(h: ChangingBatch10) { h.expect("valid").finish(); }
fn known_option(opt: Option<LeafBatch10>) { opt.unwrap().finish(); }
fn known_result(res: Result<LeafBatch10, ()>) { res.expect("valid").finish(); }
fn missing_external(path: std::path::PathBuf) { path.unwrap().exists(); path.expect("missing").exists(); }
use serde_json as json_alias;
use serde_yaml::from_str as read_yaml;
fn json_alias_payload() { json_alias::from_str::<LeafBatch10>("{}").unwrap().finish(); }
fn yaml_import_payload() { read_yaml::<LeafBatch10>("{}").unwrap().finish(); }
fn annotated_json_payload() {
    let result: Result<LeafBatch10, serde_json::Error> = serde_json::from_str("{}");
    result.unwrap().finish();
}
"#);
        let mut cases = Vec::new();
        for (package, methods) in [
            ("serde_json", &["from_str", "from_slice", "from_reader", "from_value"][..]),
            ("serde_yaml", &["from_str", "from_slice", "from_reader"][..]),
        ] {
            for method in methods {
                let function = format!("{package}_{method}");
                let reader_type = if *method == "from_reader" { "_, " } else { "" };
                let input = match *method {
                    "from_slice" => "b\"{}\"",
                    "from_reader" => "\"{}\".as_bytes()",
                    "from_value" => "serde_json::Value::Null",
                    _ => "\"{}\"",
                };
                source.push_str(&format!("fn {function}() {{ let custom = {package}::{method}::<{reader_type}LeafBatch10>({input}).unwrap(); custom.finish(); let text = {package}::{method}::<{reader_type}String>({input}).unwrap(); text.len(); let value = {package}::{method}::<{reader_type}{package}::Value>({input}).unwrap(); value.is_null(); }}\n"));
                source.push_str(&format!("fn {function}_unknown<T: serde::de::DeserializeOwned>() {{ let unknown = {package}::{method}::<{reader_type}T>({input}).unwrap(); unknown.finish(); let untyped = {package}::{method}({input}).unwrap(); untyped.finish(); }}\n"));
                source.push_str(&format!("fn {function}_direct() {{ {package}::{method}::<{reader_type}LeafBatch10>({input}).unwrap().finish(); }}\n"));
                cases.push(function);
            }
        }
        workspace.write("src/lib.rs", &source);
        let db = workspace.build();
        for function in &cases {
            let exact: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='finish' AND d.qualname LIKE '%LeafBatch10.finish' AND e.kind='calls'", [function], |row| row.get(0)).unwrap();
            assert_eq!(exact, 1, "{function}: explicit generic must retain the exact Leaf target");
            let direct: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='finish' AND d.qualname LIKE '%LeafBatch10.finish' AND e.kind='calls'", [format!("{function}_direct")], |row| row.get(0)).unwrap();
            assert_eq!(direct, 1, "{function}: direct generic chain must retain Leaf identity");
            for (expression, origin) in [("text.len", "Rust standard library"), ("value.is_null", "Value")] {
                let (status, evidence): (String, String) = db.query_row("SELECT c.status, v.evidence FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN coverage_evidence v USING(evidence_id) JOIN nodes n ON n.path_id=c.path_id AND c.line BETWEEN n.line AND n.end_line WHERE n.name=?1 AND x.expression=?2", [function.as_str(), expression], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
                assert_eq!(status, "external", "{function}: {expression}: {evidence}");
                assert!(evidence.contains(origin), "{function}: {expression}: expected {origin}, got {evidence}");
            }
            let unknown_edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='finish' AND e.kind='calls'", [format!("{function}_unknown")], |row| row.get(0)).unwrap();
            assert_eq!(unknown_edges, 0, "unknown generic/untyped results must not invent Leaf identity");
            for expression in ["unknown.finish", "untyped.finish"] {
                let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN nodes n ON n.path_id=c.path_id AND c.line BETWEEN n.line AND n.end_line WHERE n.name=?1 AND x.expression=?2", [format!("{function}_unknown").as_str(), expression], |row| row.get(0)).unwrap();
                assert_eq!(status, "unresolved", "{function}: {expression} must not manufacture Value identity");
            }
        }
        for (function, expression) in [
            ("missing_unwrap", "h.unwrap().leaf"), ("missing_expect", "h.expect(\"missing\").leaf"),
            ("missing_try", "h?.leaf"), ("assigned_missing", "changed.leaf"),
        ] {
            let exact: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='leaf' AND e.kind='calls'", [function], |row| row.get(0)).unwrap();
            assert_eq!(exact, 0, "{function}: unknown projection must not revive Hub identity");
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN nodes n ON n.path_id=c.path_id AND c.line BETWEEN n.line AND n.end_line WHERE n.name=?1 AND x.expression=?2", [function, expression], |row| row.get(0)).unwrap();
            assert_eq!(status, "unresolved", "{function}: {expression}");
        }
        for function in ["declared_unwrap", "declared_expect", "known_option", "known_result", "json_alias_payload", "yaml_import_payload", "annotated_json_payload"] {
            let exact: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='finish' AND e.kind='calls'", [function], |row| row.get(0)).unwrap();
            assert_eq!(exact, 1, "{function}: proven payload must retain Leaf identity");
        }
        for expression in ["path.unwrap().exists", "path.expect(\"missing\").exists"] {
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) WHERE x.expression=?1", [expression], |row| row.get(0)).unwrap();
            assert_eq!(status, "unresolved", "unwrapped external non-wrapper: {expression}");
        }
    }
    let empty_catalog = std::collections::BTreeMap::new();
    let seam_resolver = SeamValueFlowResolver;
    let _seam_index =
        ValueFlowIndex::build_with_limits(&empty_catalog, &seam_resolver, Default::default());
    let source = r#"
use std::collections::HashMap;

pub struct Config {
    pub host: String,
    pub port: u16,
}

pub enum Status {
    Active,
    Inactive,
}

impl Config {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}
"#;
    let facts = ast::extract("test.rs", "rust", source).expect("rust extraction failed");
    {
        use forge_index::linker::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
        use contextunity_forge_mcp::core::{models::Node, semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts}};
        struct DotConstructorResolver<'a> { class: &'a Node, constructor: &'a Node, flow: &'a ValueFlowFacts }
        impl<'a> SemanticResolver<'a> for DotConstructorResolver<'a> {
            fn resolve_symbol(&self, _: &'a Node, name: &str, _: SourcePosition, role: SymbolRole) -> Symbol<'a> {
                match (name, role) {
                    ("Config", SymbolRole::Type) => Symbol::Type(TypeTarget::Local(self.class)),
                    _ => Symbol::Unknown,
                }
            }
            fn resolve_member(&self, receiver: &TypeTarget<'a>, member: &str, _: &'a Node, _: SourcePosition) -> Symbol<'a> {
                if matches!(receiver, TypeTarget::Local(node) if node.id == self.class.id) && member == "new" {
                    Symbol::Callable(self.constructor)
                } else { Symbol::Unknown }
            }
            fn value_flow(&self, node: &'a Node) -> Option<&'a ValueFlowFacts> {
                (node.id == self.constructor.id).then_some(self.flow)
            }
        }
        let class = facts.nodes.iter().find(|node| node.kind == "struct" && node.name == "Config").unwrap();
        let constructor = facts.nodes.iter().find(|node| node.kind == "method" && node.name == "new").unwrap();
        let flow = ValueFlowFacts { return_type: Some(TypeExpr::Named { name: "Self".to_owned() }), ..Default::default() };
        let resolver = DotConstructorResolver { class, constructor, flow: &flow };
        let index = ValueFlowIndex::build(&empty_catalog, &resolver);
        for callee in ["Config::new", "Config.new", "Config::create"] {
            let result = index.resolve_value(class, &ValueExpr::Call { callee: callee.to_owned(), site: None }, SourcePosition { line: 20, column: 0 }, &resolver);
            if callee == "Config::create" {
                assert!(matches!(result, TypeTarget::Unknown), "missing constructors must stay unknown: {result:?}");
            } else {
                assert!(matches!(result, TypeTarget::Local(node) if node.id == class.id), "constructor spelling must normalize: {callee}: {result:?}");
            }
        }
    }
    {
        let source = r#"
struct LeafBatch9 {}
impl LeafBatch9 { fn finish(&self) {} }
struct HubBatch9 {}
impl HubBatch9 { fn leaf(&self) -> LeafBatch9 { LeafBatch9 {} } }
fn probe(name: &str, path: &std::path::PathBuf, hub: HubBatch9, items: Vec<LeafBatch9>) {
    name.trim().is_empty();
    path.parent().unwrap().exists();
    hub.leaf().finish();
    hub.missing().finish();
    hub.leaf(unknown).finish();
    for mut item in items { item.finish(); }
    let mut bound = LeafBatch9 {}; bound.finish();
}
"#;
        let mut facts = ast::extract("batch9.rs", "rust", source).unwrap();
        for reference in &mut facts.references { reference.receiver_hint = None; }
        let graph = linker::link(&std::collections::BTreeMap::from([("batch9.rs".into(), facts)]));
        for (expression, expected) in [
            ("name.trim().is_empty", true), ("path.parent().unwrap().exists", true),
            ("hub.leaf().finish", true), ("hub.missing().finish", false),
            ("hub.leaf(unknown).finish", false), ("item.finish", true), ("bound.finish", true),
        ] {
            let row = graph.coverage.iter().find(|row| row.expression == expression).unwrap();
            assert_eq!(matches!(row.status.as_str(), "resolved" | "external"), expected, "{expression}: {row:?}");
        }
    }
    {
        use forge_index::linker::value_flow::{ConfiguredReceiverAuthority, SemanticResolver, Symbol, SymbolRole, TypeTarget};
        use contextunity_forge_mcp::core::{models::Node, semantic::{SourcePosition, TypeExpr, ValueExpr}};
        use forge_index::languages::manifests::{ClassHierarchyReceiverRule, ReturnPolicy};
        fn configured(base: &str, member: &str, returns: bool) -> TypeTarget<'static> {
            TypeTarget::ConfiguredReceiver {
                authority: std::sync::Arc::new(ConfiguredReceiverAuthority::Hierarchy {
                    pack: "supplied-rust-policy".to_owned(),
                    rule: std::sync::Arc::new(ClassHierarchyReceiverRule {
                        bases: vec![base.to_owned()], provider: "supplied".to_owned(),
                        members: vec![member.to_owned()],
                        returns: if returns {
                            std::collections::BTreeMap::from([(member.to_owned(), ReturnPolicy::Receiver { base: "Replacement".to_owned() })])
                        } else { Default::default() },
                    }),
                }),
                origin: Box::new(TypeTarget::External { module: base.to_owned(), import_line: 1 }),
                fallback: None, evidence: "supplied-policy".to_owned(),
            }
        }
        struct SuppliedResolver { receiver: TypeTarget<'static>, result: Option<TypeTarget<'static>> }
        impl<'a> SemanticResolver<'a> for SuppliedResolver {
            fn resolve_symbol(&self, _: &'a Node, name: &str, _: SourcePosition, _: SymbolRole) -> Symbol<'a> {
                if name == "Supplied" { Symbol::Type(self.receiver.clone()) } else { Symbol::Unknown }
            }
            fn resolve_member_result(&self, receiver: &TypeTarget<'a>, member: &str, _: &'a Node, _: SourcePosition) -> Option<TypeTarget<'a>> {
                let TypeTarget::ConfiguredReceiver { authority, .. } = receiver else { return None; };
                let ConfiguredReceiverAuthority::Hierarchy { rule, .. } = authority.as_ref() else { return None; };
                rule.returns.get(member)?;
                self.result.clone()
            }
        }
        let owner = facts.nodes.iter().find(|node| node.kind == "method").unwrap();
        let at = SourcePosition { line: owner.line + 1, column: 0 };
        for (base, member, wrappers) in [
            ("rusqlite::Rows", "next", 2),
            ("rusqlite::Row", "get", 1),
            ("std::process::Command", "output", 1),
            ("walkdir::WalkDir", "into_iter", 1),
        ] {
            let expression = ValueExpr::Project {
                value: Box::new(ValueExpr::Annotated { type_expr: TypeExpr::Named { name: "Supplied".to_owned() } }),
                member: member.to_owned(),
            };
            for returns in [true, false] {
                let resolver = SuppliedResolver { receiver: configured(base, member, returns), result: Some(TypeTarget::Builtin("Replacement".to_owned())) };
                let index = ValueFlowIndex::build(&empty_catalog, &resolver);
                let mut result = index.resolve_value(owner, &expression, at, &resolver);
                if !returns { assert!(matches!(result, TypeTarget::Unknown), "{base}.{member}: {result:?}"); continue; }
                if member == "into_iter" {
                    let TypeTarget::RustIterator { item } = result else { panic!("{base}.{member}: {result:?}"); };
                    result = *item;
                }
                for depth in 0..wrappers {
                    let TypeTarget::RustWrapper { wrapper, ok, .. } = result else { panic!("{base}.{member}: {result:?}"); };
                    assert_eq!(wrapper, if depth == 1 { forge_index::linker::value_flow::RustWrapperKind::Option } else { forge_index::linker::value_flow::RustWrapperKind::Result });
                    result = *ok;
                }
                assert!(matches!(result, TypeTarget::Builtin(name) if name == "Replacement"), "{base}.{member}");
            }
        }
        let resolver = SuppliedResolver { receiver: configured("rusqlite::Row", "get", true), result: Some(configured("std::result::Result", "unwrap", false)) };
        let index = ValueFlowIndex::build(&empty_catalog, &resolver);
        let projected = index.resolve_value(owner, &ValueExpr::Project {
            value: Box::new(ValueExpr::Annotated { type_expr: TypeExpr::Named { name: "Supplied".to_owned() } }),
            member: "get".to_owned(),
        }, at, &resolver);
        assert!(matches!(projected, TypeTarget::RustWrapper { ok, .. } if matches!(*ok, TypeTarget::Unknown)));
    }
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "struct" && n.name == "Config"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "enum" && n.name == "Status"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "new"));
    let constructor = facts
        .nodes
        .iter()
        .find(|n| n.name == "new")
        .expect("new method missing");
    assert_eq!(constructor.details["receiver_type"], "Config");
    assert!(constructor.details.get("is_method").is_none());
    assert_eq!(constructor.details["is_static"], true);

    #[cfg(feature = "lang-rust")]
    {
        let rust_kind = kind("function_item");
        assert_eq!(rust_kind, Some("function"));
        let _ = rust::language();
        let _ = is_registered_cargo_crate("rusqlite");
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_std_prelude_and_trait_manifest() {
    // Contract baseline for LS-001: Rust std prelude trait resolution and declarative manifests.
    // Standard prelude traits (Clone, Default, Iterator) must resolve or classify as external via rust-std.toml.
    let std_source = r#"
fn run() {
    let _default: Result<i32, ()> = Result::default();
    let text = String::from("hello");
    let _cloned = text.clone();
    let _default_trait: i32 = Default::default();
    let numbers = Vec::new();
    let _iter = numbers.iter();
}
"#;
    let std_facts =
        ast::extract("std_prelude.rs", "rust", std_source).expect("std_prelude extraction failed");
    let std_graph = linker::link(&std::collections::BTreeMap::from([(
        "std_prelude.rs".to_owned(),
        std_facts,
    )]));
    let default_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Result::default")
        .expect("missing coverage for Result::default");
    assert_eq!(
        default_coverage.status, "unresolved",
        "Result does not implement Default: {default_coverage:#?}"
    );
    let string_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "String::from")
        .expect("missing coverage for String::from");
    assert_eq!(
        string_coverage.status, "external",
        "String::from must resolve to external Rust standard library prelude via rust-std.toml: {string_coverage:#?}"
    );
    let clone_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "text.clone")
        .expect("missing coverage for text.clone");
    assert_eq!(
        clone_coverage.status, "external",
        "text.clone must resolve to external Rust standard library prelude via rust-std.toml: {clone_coverage:#?}"
    );
    let default_trait_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Default::default")
        .expect("missing coverage for Default::default");
    assert_eq!(
        default_trait_coverage.status, "external",
        "Default::default must resolve to external Rust standard library prelude via rust-std.toml: {default_trait_coverage:#?}"
    );
    let vec_new_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Vec::new")
        .expect("missing coverage for Vec::new");
    assert_eq!(
        vec_new_coverage.status, "external",
        "Vec::new must resolve to external: {vec_new_coverage:#?}"
    );
    let iter_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "numbers.iter")
        .expect("missing coverage for numbers.iter");
    assert_eq!(
        iter_coverage.status, "external",
        "numbers.iter must resolve to external: {iter_coverage:#?}"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_cargo_ecosystem_manifest_and_external_routing() {
    // Verify Cargo third-party ecosystem crate routing (Task 2 contract)
    let cargo_source = r#"
use rusqlite::Connection;
use tree_sitter::Parser;

fn run_cargo_crates() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("SELECT 1", []).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
}
"#;
    // Cargo authority follows the imported identity and declared member, never a
    // homonymous short type name or an assumed constructor convention.
    for (label, import, constructor, member, authorized) in [
        (
            "canonical",
            "use rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            true,
        ),
        (
            "alias",
            "use rusqlite::Connection as Db;",
            "Db::open_in_memory()",
            "execute",
            true,
        ),
        (
            "declared",
            "use rusqlite::Connection;",
            "Connection::open_with_flags(\"db\", flags)",
            "execute",
            true,
        ),
        (
            "homonym",
            "use unregistered_crate::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "suffix",
            "use unregistered_crate::rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "undeclared",
            "use rusqlite::Connection;",
            "Connection::with_capacity(12)",
            "execute",
            false,
        ),
        (
            "std-file-result",
            "use std::fs::File;",
            "File::open(\"input\")",
            "metadata",
            true,
        ),
        (
            "builtin-string-result",
            "use std::str::FromStr;",
            "String::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "imported-string-result",
            "use std::string::String as Text; use std::str::FromStr;",
            "Text::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "tempfile-named",
            "use tempfile::NamedTempFile;",
            "NamedTempFile::new()",
            "path",
            true,
        ),
        (
            "clap-command",
            "use clap::Command;",
            "Command::new(\"app\")",
            "get_matches",
            true,
        ),
        (
            "clap-command-missing-unwrap",
            "use clap::Command;",
            "Command::new(\"app\")",
            "get_matches",
            false,
        ),
        (
            "regex-match",
            "use regex::Regex;",
            "Regex::new(\"pattern\")",
            "is_match",
            true,
        ),
    ] {
        let projection = if label == "clap-command" { "" } else { ".unwrap()" };
        let source =
            format!("{import}\nfn run() {{ let conn = {constructor}{projection}; conn.{member}(); }}");
        for path in ["identity.rs", "src/identity.rs"] {
            let facts = ast::extract(path, "rust", &source).unwrap();
            let graph = linker::link(&std::collections::BTreeMap::from([(path.into(), facts)]));
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == format!("conn.{member}"))
                .unwrap();
            assert_eq!(
                coverage.status == "external",
                authorized,
                "{label} at {path}: {coverage:#?}"
            );
            if authorized {
                assert_eq!(
                    coverage.status, "external",
                    "{label} at {path}: {coverage:#?}"
                );
            }
        }
    }
    // Prove negative isolation: conflicting imported alias and explicitly local module shadowing
    for (label, source, expected_expr) in [
        (
            "duplicate-rust-alias",
            "use regex::Regex as Re; use unregistered_crate::Other as Re; fn run() { let r = Re::new(\"x\").unwrap(); r.is_match(\"x\"); }",
            "r.is_match",
        ),
        (
            "local-missing-type",
            "mod regex {} fn run(r: regex::Missing) { r.is_match(\"x\"); }",
            "r.is_match",
        ),
    ] {
        for path in ["negative.rs", "src/negative.rs"] {
            let facts = ast::extract(path, "rust", source).unwrap();
            let graph = linker::link(&std::collections::BTreeMap::from([(path.into(), facts)]));
            let coverage = graph.coverage.iter().find(|row| row.expression == expected_expr);
            assert!(
                coverage.is_none() || coverage.unwrap().status == "unresolved",
                "{label} at {path}: expected unresolved, got {coverage:#?}"
            );
        }
    }

    let cargo_facts = ast::extract("cargo_test.rs", "rust", cargo_source).unwrap();
    let cargo_graph = linker::link(&std::collections::BTreeMap::from([(
        "cargo_test.rs".to_owned(),
        cargo_facts,
    )]));
    let execute_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "conn.execute")
        .expect("missing coverage for conn.execute");
    assert_eq!(
        execute_cov.status, "external",
        "conn.execute must resolve to external via rust-cargo.toml declarative pack: {execute_cov:#?}"
    );
    assert!(
        execute_cov.evidence.contains("rusqlite::Connection")
            && execute_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "conn.execute must retain its resolved import origin: {execute_cov:#?}"
    );
    let parser_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "parser.set_language")
        .expect("missing coverage for parser.set_language");
    assert_eq!(
        parser_cov.status, "external",
        "parser.set_language must resolve to external via rust-cargo.toml declarative pack: {parser_cov:#?}"
    );
    assert!(
        parser_cov.evidence.contains("tree_sitter::Parser")
            && parser_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "parser.set_language must retain its resolved import origin: {parser_cov:#?}"
    );

    let iso_source = r#"
fn check_string_trait_isolation() {
    let s = String::new();
    s.clone();
    s.next();
}
"#;
    let iso_facts = ast::extract("iso_test.rs", "rust", iso_source).unwrap();
    let iso_graph = linker::link(&std::collections::BTreeMap::from([(
        "iso_test.rs".to_owned(),
        iso_facts,
    )]));
    let next_cov = iso_graph
        .coverage
        .iter()
        .find(|c| c.expression == "s.next")
        .expect("missing coverage for s.next");
    assert_eq!(
        next_cov.status, "unresolved",
        "s.next must remain unresolved because String does not implement Iterator: {next_cov:#?}"
    );
    for (adapter, preserves_item) in [
        ("filter(|_| true)", true),
        ("take(1)", true),
        ("skip(1)", true),
        ("step_by(1)", true),
        ("collect::<Vec<_>>().into_iter()", true),
        ("map(|_| B)", false),
        ("map(convert)", false),
        ("enumerate()", false),
        ("map(|item| item)", false),
    ] {
        let source = format!(
            "struct A; impl A {{ fn finish(&self) {{}} }}
             struct B; impl B {{ fn finish(&self) {{}} }}
             fn convert(_: A) -> B {{ B }}
             trait Finish {{ fn finish(&self); }}
             impl Finish for (usize, A) {{ fn finish(&self) {{}} }}
             fn run(items: Vec<A>) {{
                 let mut adapted = items.into_iter().{adapter};
                 let item = adapted.next().unwrap();
                 item.finish();
             }}"
        );
        let facts = ast::extract("iterator.rs", "rust", &source).unwrap();
        assert!(facts.errors.is_empty(), "{adapter}: {facts:#?}");
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "finish" && node.details["receiver_type"] == "A")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "iterator.rs".to_owned(),
            facts,
        )]));
        assert_eq!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == "item.finish"
                && edge.dst == target),
            preserves_item,
            "{adapter}: {graph:#?}"
        );
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == "item.finish")
            .unwrap();
        assert_eq!(
            coverage.status,
            if preserves_item {
                "resolved"
            } else {
                "unresolved"
            },
            "{adapter}: {coverage:#?}"
        );
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_declarative_macros_and_receiver_repair() {
    // Task 3 Contract: Declarative macro classification and bounded receiver repair (LS-001)
    // 1. Declarative standard and Cargo macro classification: format!, json!, bail!, ensure!, vec!, write!
    let macro_contract_source = r#"
use anyhow::bail;
use anyhow::ensure;
use serde_json::json;

fn run_macros() {
    let _f = format!("hello {}", 42);
    let _v = vec![1, 2, 3];
    let _w = write!(f, "data");
    let _j = json!({ "key": "value" });
    let _b = bail!("operation failed");
    let _e = ensure!(true);
    let _qb = anyhow::bail!("qualified bail");
    let _qe = anyhow::ensure!(true);
    let _qj = serde_json::json!({ "status": 200 });
}
"#;
    let macro_facts = ast::extract("macros.rs", "rust", macro_contract_source).unwrap();
    let macro_graph = linker::link(&std::collections::BTreeMap::from([(
        "macros.rs".to_owned(),
        macro_facts,
    )]));
    for macro_name in [
        "format",
        "vec",
        "write",
        "json",
        "bail",
        "ensure",
        "anyhow::bail",
        "anyhow::ensure",
        "serde_json::json",
    ] {
        let cov = macro_graph
            .coverage
            .iter()
            .find(|c| c.expression == macro_name)
            .unwrap_or_else(|| panic!("missing coverage for macro {macro_name}"));
        assert_ne!(
            cov.status, "unresolved",
            "macro {macro_name} must not be unresolved: {cov:#?}"
        );
        assert_eq!(
            cov.status, "external",
            "macro {macro_name} status mismatch: {cov:#?}"
        );
    }

    // Unimported Cargo ecosystem macros (json!, bail!, ensure!) must classify via declarative macro signatures
    let unimported_macro_source = r#"
fn run_unimported_macros() {
    let _j = json!({ "key": "value" });
    let _b = bail!("operation failed");
    let _e = ensure!(true);
}
"#;
    let unimported_facts =
        ast::extract("unimported_macros.rs", "rust", unimported_macro_source).unwrap();
    let unimported_graph = linker::link(&std::collections::BTreeMap::from([(
        "unimported_macros.rs".to_owned(),
        unimported_facts,
    )]));
    for name in ["json", "bail", "ensure"] {
        let cov = unimported_graph
            .coverage
            .iter()
            .find(|c| c.expression == name)
            .unwrap_or_else(|| panic!("missing coverage for unimported {name}"));
        assert_ne!(
            cov.status, "unresolved",
            "unimported macro {name} must be classified through declarative macro signatures: {cov:#?}"
        );
    }

    // Imported local macros (named, renamed, crate) must not manufacture Vec/String stdlib receiver provenance
    for name in ["vec", "format"] {
        for (label, import, definition) in [
            (
                "named",
                format!("use local::{name};"),
                format!("macro_rules! {name} {{ () => {{ A }}; }} pub(crate) use {name};"),
            ),
            (
                "renamed",
                format!("use local::custom as {name};"),
                "macro_rules! custom { () => { A }; } pub(crate) use custom;".to_owned(),
            ),
            (
                "crate",
                format!("use crate::local::{name};"),
                format!("macro_rules! {name} {{ () => {{ A }}; }} pub(crate) use {name};"),
            ),
        ] {
            let source = format!(
                "struct A; mod local {{ {definition} }} {import} fn run() {{ let item = {name}!(); item.len(); }}"
            );
            let facts = ast::extract("probe.rs", "rust", &source).unwrap();
            assert!(
                facts.errors.is_empty(),
                "{name}/{label}: {:?}",
                facts.errors
            );
            let graph = linker::link(&std::collections::BTreeMap::from([(
                "probe.rs".to_owned(),
                facts,
            )]));
            let row = graph
                .coverage
                .iter()
                .find(|r| r.expression == "item.len")
                .unwrap();
            assert_ne!(
                row.status, "external",
                "local imported macro {name}/{label} fabricated stdlib type: {row:?}"
            );
        }
    }

    // 2. Bounded iterator receiver propagation: chained .iter().filter().map() must bind intermediate receivers and resolve .collect()
    let chain_source = r#"
fn run_iterator_chain() {
    let numbers = Vec::new();
    let _collected = numbers.into_iter().filter(|x| true).map(|x| x).collect();
    let _deep = numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect();
    let _from_macro = vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect();
}
"#;
    let chain_facts = ast::extract("chain.rs", "rust", chain_source).unwrap();
    let chain_graph = linker::link(&std::collections::BTreeMap::from([(
        "chain.rs".to_owned(),
        chain_facts,
    )]));
    for expr in [
        "numbers.into_iter().filter(|x| true).map(|x| x).collect",
        "numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect",
        "vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect",
    ] {
        let collect_cov = chain_graph
            .coverage
            .iter()
            .find(|c| c.expression == expr)
            .unwrap_or_else(|| panic!("missing coverage for {expr}"));
        assert_ne!(
            collect_cov.status, "unresolved",
            "collect() on chained iterator receiver must not be unresolved: {collect_cov:#?}"
        );
    }

    // Verify R2 remediation: ordinary function calls json() without macro ! must remain unresolved
    let fn_call_source = r#"
fn run_unresolved_function() {
    let _call = json();
}
"#;
    let fn_call_facts = ast::extract("fn_call.rs", "rust", fn_call_source).unwrap();
    let fn_call_graph = linker::link(&std::collections::BTreeMap::from([(
        "fn_call.rs".to_owned(),
        fn_call_facts,
    )]));
    let fn_cov = fn_call_graph
        .coverage
        .iter()
        .find(|c| c.expression == "json")
        .expect("missing coverage for json call");
    assert_eq!(
        fn_cov.status, "unresolved",
        "ordinary function call json() must remain unresolved: {fn_cov:#?}"
    );

    // Verify R1 boundary: 7 and 8 calls propagate, while 9+ calls exhaust and leave collect() unresolved
    let over_eight_source = r#"
fn run_over_eight_chain() {
    let numbers = Vec::new();
    let _call7 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).collect();
    let _call8 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).collect();
    let _over9 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).collect();
    let _over10 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect();
    let _over19 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect();
}
"#;
    let over_eight_facts = ast::extract("over_eight.rs", "rust", over_eight_source).unwrap();
    let over_eight_graph = linker::link(&std::collections::BTreeMap::from([(
        "over_eight.rs".to_owned(),
        over_eight_facts,
    )]));
    for (expr, expected) in [
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
    ] {
        let cov = over_eight_graph
            .coverage
            .iter()
            .find(|c| c.expression == expr)
            .unwrap_or_else(|| panic!("missing coverage for {expr}"));
        assert_eq!(
            cov.status, expected,
            "chain call {expr} expected status {expected}: {cov:#?}"
        );
    }

    // Verify R4: local take() returning another type must NOT preserve original receiver across iterator spellings
    for iter_name in ["iter", "into_iter", "iter_mut"] {
        for adapter in ["take(1)", "skip(1)", "step_by(1)", "filter(|_| true)"] {
            let r4_source = format!(
                "struct A; struct B;
                 impl A {{
                     fn {iter_name}(&self) -> A {{ A }}
                     fn take(&self, _: usize) -> B {{ B }}
                     fn skip(&self, _: usize) -> B {{ B }}
                     fn step_by(&self, _: usize) -> B {{ B }}
                     fn filter(&self, _: fn(&A) -> bool) -> B {{ B }}
                     fn finish(&self) {{}}
                 }}
                 fn run(a: A) {{
                     let item = a.{iter_name}().{adapter};
                     item.finish();
                 }}"
            );
            let r4_facts = ast::extract("r4.rs", "rust", &r4_source).unwrap();
            let r4_graph = linker::link(&std::collections::BTreeMap::from([(
                "r4.rs".to_owned(),
                r4_facts,
            )]));
            let r4_cov = r4_graph
                .coverage
                .iter()
                .find(|c| c.expression == "item.finish")
                .expect("missing coverage for item.finish");
            assert_ne!(
                r4_cov.status, "resolved",
                "item.finish on non-iterator B from {iter_name}.{adapter} must not resolve to A.finish: {r4_cov:#?}"
            );
        }
    }

    // Verify R2: local macro_rules! definition must preserve local identity and not become external serde_json
    let local_macro_source = r#"
macro_rules! json {
    () => { 1 };
}
fn run_local_macro() {
    let _val = json!();
}
"#;
    let local_macro_facts = ast::extract("local_macro.rs", "rust", local_macro_source).unwrap();
    let local_macro_graph = linker::link(&std::collections::BTreeMap::from([(
        "local_macro.rs".to_owned(),
        local_macro_facts,
    )]));
    let local_cov = local_macro_graph
        .coverage
        .iter()
        .find(|c| c.expression == "json")
        .expect("missing coverage for local json call");
    assert_ne!(
        local_cov.status, "external",
        "local macro json!() must not be classified as external serde_json: {local_cov:#?}"
    );

    // Verify R2: standard macro spellings called as ordinary functions must remain unresolved
    let std_fn_calls_source = r#"
fn run_std_fn_calls() {
    let _f = format();
    let _v = vec();
    let _w = write();
    let _p = println();
}
"#;
    let std_fn_calls_facts = ast::extract("std_fn_calls.rs", "rust", std_fn_calls_source).unwrap();
    let std_fn_calls_graph = linker::link(&std::collections::BTreeMap::from([(
        "std_fn_calls.rs".to_owned(),
        std_fn_calls_facts,
    )]));
    for name in ["format", "vec", "write", "println"] {
        let std_cov = std_fn_calls_graph
            .coverage
            .iter()
            .find(|c| c.expression == name)
            .unwrap_or_else(|| panic!("missing coverage for std fn call {name}"));
        assert_eq!(
            std_cov.status, "unresolved",
            "ordinary function call {name}() must remain unresolved: {std_cov:#?}"
        );
    }

    // Verify R2 boundary tests: local macro imports/aliases precede external names
    for (label, source, expression) in [
        ("qualified", "mod serde_json { macro_rules! json { () => { 1 }; } pub(crate) use json; } fn run() { serde_json::json!(); }", "serde_json::json"),
        ("import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use local::json; fn run() { json!(); }", "json"),
        ("renamed-import", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local::custom as json; fn run() { json!(); }", "json"),
        ("crate-import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use crate::local::json; fn run() { json!(); }", "json"),
        ("module-alias", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom as json; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
        ("parent-module", "mod outer { macro_rules! json { () => { 1 }; } mod inner { fn run() { json!(); } } }", "json"),
        ("parent-control", "mod outer { macro_rules! custom { () => { 1 }; } mod inner { fn run() { custom!(); } } }", "custom"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == expression).unwrap();
        assert_ne!(row.status, "external", "{label}: local macro misclassified external: {row:?}");
        assert_eq!(row.status, "resolved", "{label}: local macro should resolve locally: {row:?}");
    }

    // Verify R2-output: local macro result must not fabricate stdlib type
    for name in ["vec", "format"] {
        let source = format!("struct A; impl A {{ fn finish(&self) {{}} }} macro_rules! {name} {{ () => {{ A }}; }} fn run() {{ let item = {name}!(); item.len(); }}");
        let facts = ast::extract("probe.rs", "rust", &source).unwrap();
        assert!(facts.errors.is_empty(), "{:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        let row = graph
            .coverage
            .iter()
            .find(|r| r.expression == "item.len")
            .unwrap();
        assert_ne!(
            row.status, "external",
            "local {name} result must not fabricate stdlib type: {row:?}"
        );
    }

    // Verify qualified logging macro providers come from TOML
    {
        let source = "fn run() { tracing::info!(\"hello\"); log::info!(\"hello\"); }";
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        for provider in ["tracing", "log"] {
            let expression = format!("{provider}::info");
            let row = graph
                .coverage
                .iter()
                .find(|r| r.expression == expression)
                .unwrap();
            assert_eq!(row.status, "external");
            assert!(row.evidence.contains(&format!("macro {provider}::")));
        }
    }

    // Verify Review 42: nonexistent import member does not select different member
    for (label, source, expression) in [
        ("direct-import", "mod local { pub fn available() {} } use local::missing; fn run() { missing(); }", "missing"),
        ("module-alias", "mod local { pub fn available() {} } use local as alias; fn run() { alias::missing(); }", "alias::missing"),
        ("macro-member", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == expression).unwrap();
        assert_ne!(row.status, "resolved", "{label}: invented member should not be resolved: {row:?}");
    }

    // Verify Review 42: scoped macro import does not leak to unrelated outer scope
    {
        let source = "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } mod child { use crate::local::custom as json; fn scoped() { json!(); } }\nfn run() { json!(); }";
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        let row = graph
            .coverage
            .iter()
            .filter(|r| r.expression == "json")
            .max_by_key(|r| r.line)
            .unwrap();
        assert_ne!(
            row.status, "resolved",
            "child import must not supply exact target for outer json!"
        );
    }

    // Verify Review 42: unrelated macro or value import does not suppress std macro result
    for (label, source) in [
        ("sibling-macro", "mod other { macro_rules! vec { () => { 1 }; } } fn run() { let item = vec![1]; item.len(); }"),
        ("value-namespace", "use std::fmt::format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("later-macro", "fn run() { let item = vec![1]; item.len(); } macro_rules! vec { () => { 1 }; }"),
        ("foreign-function", "mod local { extern \"C\" { pub fn custom(); } } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
        ("foreign-static", "mod local { extern \"C\" { pub static custom: u8; } } use local::custom as format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("extern-crate", "mod local { pub extern crate core as custom; } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == "item.len").unwrap();
        assert_eq!(row.status, "external", "{label}: proven std macro output suppressed: {row:?}");
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_closure_inference_and_tuple_destructuring() {
    // Seam Contract: closure-inference-and-tuple-destructuring (LS-001 Task 5)
    // 1. Tuple destructuring: let (file, code) = (NamedTempFile::new().unwrap(), 42); file.path();
    // 2. Additional workspace Cargo crates: WalkDir::new(".").into_iter();
    // 3. Closure parameter type inference: query_map(..., |row| row.get(...))
    assert!(
        contextunity_forge_mcp::engine::languages::rust::is_registered_rust_type(
            "walkdir::WalkDir"
        )
    );
    let seam_source = r#"
        use tempfile::NamedTempFile;
        use walkdir::WalkDir;
        use rusqlite::Connection;
        fn run_seam(conn: &Connection) {
            let (file, code) = (NamedTempFile::new().unwrap(), 42);
            file.path();
            let walker = WalkDir::new(".").into_iter();
            let mut stmt = conn.prepare("SELECT 1").unwrap();
            let _ = stmt.query_map([], |row| {
                row.get::<_, i64>(0)
            });
        }
        fn run_match_and_if_let() {
            let conn_res = Connection::open_in_memory();
            let conn = match conn_res {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Ok(mut stmt) = conn.prepare("SELECT 1") {
                stmt.execute([]).unwrap();
            }
        }
        fn run_root_crate_import() {
            use contextunity_forge_mcp::engine::ast;
            let _ = ast::extract("test.rs", "rust", "fn main() {}");
        }
    "#;
    let seam_facts = ast::extract("src/seam_destructuring.rs", "rust", seam_source).unwrap();
    let ast_engine_facts = ast::extract(
        "src/engine/ast.rs",
        "rust",
        "pub fn extract(_path: &str, _lang: &str, _code: &str) {}",
    )
    .unwrap();
    let seam_graph = linker::link(&std::collections::BTreeMap::from([
        ("src/seam_destructuring.rs".into(), seam_facts),
        ("src/engine/ast.rs".into(), ast_engine_facts),
    ]));
    let file_path_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "file.path")
        .expect("missing coverage for file.path");
    assert_eq!(
        file_path_cov.status, "external",
        "file.path from tuple destructuring must resolve to external: {file_path_cov:#?}"
    );
    let row_get_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression.starts_with("row.get"))
        .expect("missing coverage for row.get");
    assert_eq!(
        row_get_cov.status, "external",
        "row.get from closure inference must resolve to external: {row_get_cov:#?}"
    );
    let stmt_exec_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "stmt.execute")
        .expect("missing coverage for stmt.execute");
    assert_eq!(
        stmt_exec_cov.status, "external",
        "stmt.execute from if-let Ok(mut stmt) pattern binding must resolve to external: {stmt_exec_cov:#?}"
    );
    let ast_extract_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "ast::extract")
        .expect("missing coverage for ast::extract");
    assert_eq!(
        ast_extract_cov.status, "external",
        "ast::extract without owning Cargo manifest and indexed src/lib.rs must remain external: {ast_extract_cov:#?}"
    );

    // Proven manifest-bound root crate normalization
    let unique_id = format!(
        "forge_test_root_crate_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_root = std::env::temp_dir().join(unique_id);
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();
    std::fs::write(
        temp_root.join("Cargo.toml"),
        r#"[package]
name = "my_root_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(temp_root.join("src")).unwrap();
    std::fs::write(temp_root.join("src/lib.rs"), "pub mod engine;").unwrap();

    let root_caller_source = r#"
        use my_root_crate::engine::ast;
        fn run() {
            let _ = ast::extract("test.rs", "rust", "fn main() {}");
        }
    "#;
    let caller_facts = ast::extract("src/main.rs", "rust", root_caller_source).unwrap();
    let engine_ast_facts = ast::extract(
        "src/engine/ast.rs",
        "rust",
        "pub fn extract(_path: &str, _lang: &str, _code: &str) {}",
    )
    .unwrap();
    let lib_facts = ast::extract("src/lib.rs", "rust", "pub mod engine;").unwrap();
    let manifest_graph = linker::link_with_root(
        &std::collections::BTreeMap::from([
            ("src/main.rs".into(), caller_facts),
            ("src/engine/ast.rs".into(), engine_ast_facts),
            ("src/lib.rs".into(), lib_facts),
        ]),
        None,
        Some(&temp_root),
    );
    let resolved_cov = manifest_graph
        .coverage
        .iter()
        .find(|c| c.expression == "ast::extract")
        .expect("missing coverage for ast::extract with manifest");
    assert_eq!(
        resolved_cov.status, "resolved",
        "ast::extract with valid manifest and src/lib.rs must resolve locally: {resolved_cov:#?}"
    );
    let _ = std::fs::remove_dir_all(&temp_root);

    // Probe tests for LS001-T5 review findings R3, R4, R5
    // 1. Iterator closure inference from receiver
    let iter_source = r#"
        struct Item; impl Item { fn finish(&self) {} }
        fn f(items: Vec<Item>) {
            items.iter().map(|entry| entry.finish());
            items.iter().filter(|entry| { entry.finish(); true });
        }
    "#;
    let iter_facts = ast::extract("src/probe_iter.rs", "rust", iter_source).unwrap();
    let iter_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_iter.rs".into(),
        iter_facts,
    )]));
    for c in iter_graph
        .coverage
        .iter()
        .filter(|c| c.expression == "entry.finish")
    {
        assert_eq!(
            c.status, "resolved",
            "iterator entry.finish must resolve: {c:#?}"
        );
    }

    // 2. Tuple-return positional binding
    let tuple_ret_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn close(&self) {} }
        fn make() -> (A, B) { (A, B) }
        fn f() { let (a, b) = make(); a.finish(); b.close(); }
    "#;
    let tuple_ret_facts = ast::extract("src/probe_tup_ret.rs", "rust", tuple_ret_source).unwrap();
    let tuple_ret_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_ret.rs".into(),
        tuple_ret_facts,
    )]));
    let a_finish = tuple_ret_graph
        .coverage
        .iter()
        .find(|c| c.expression == "a.finish")
        .expect("a.finish");
    assert_eq!(
        a_finish.status, "resolved",
        "a.finish from tuple return must resolve: {a_finish:#?}"
    );
    let b_close = tuple_ret_graph
        .coverage
        .iter()
        .find(|c| c.expression == "b.close")
        .expect("b.close");
    assert_eq!(
        b_close.status, "resolved",
        "b.close from tuple return must resolve: {b_close:#?}"
    );

    // 3. Tuple-rest positional binding
    let tuple_rest_source = r#"
        struct A;
        struct B; impl B { fn finish(&self) {} }
        struct C; impl C { fn finish(&self) {} }
        fn f() { let (.., last) = (A {}, B {}, C {}); last.finish(); }
    "#;
    let tuple_rest_facts =
        ast::extract("src/probe_tup_rest.rs", "rust", tuple_rest_source).unwrap();
    let tuple_rest_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_rest.rs".into(),
        tuple_rest_facts,
    )]));
    let last_finish = tuple_rest_graph
        .coverage
        .iter()
        .find(|c| c.expression == "last.finish")
        .expect("last.finish");
    assert_eq!(
        last_finish.status, "resolved",
        "last.finish must resolve: {last_finish:#?}"
    );
    assert!(
        last_finish.evidence.contains("src.probe_tup_rest.C.finish"),
        "last.finish must resolve to C.finish, not B.finish: {last_finish:#?}"
    );

    // 4. Closure-homonym negative (unrelated callback must not resolve to rusqlite::Row)
    let closure_homonym_source = r#"
        fn unrelated<F: Fn(i32)>(callback: F) { callback(1); }
        fn f() { unrelated(|row| row.get(0)); }
    "#;
    let closure_homonym_facts =
        ast::extract("src/probe_closure_neg.rs", "rust", closure_homonym_source).unwrap();
    let closure_homonym_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_closure_neg.rs".into(),
        closure_homonym_facts,
    )]));
    let row_get = closure_homonym_graph
        .coverage
        .iter()
        .find(|c| c.expression == "row.get")
        .expect("row.get");
    assert_ne!(
        row_get.status, "external",
        "unrelated callback named row must NOT resolve to rusqlite::Row: {row_get:#?}"
    );

    // 5. Connection and statement query_map closure parameter inference
    let conn_source = r#"
        use rusqlite::Connection;
        fn f(conn: &Connection) {
            let mut statement = conn.prepare("SELECT 1").unwrap();
            statement.execute([]);
            statement.query_map([], |entry| entry.get(0));
        }
    "#;
    let conn_facts = ast::extract("src/probe_conn.rs", "rust", conn_source).unwrap();
    let conn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_conn.rs".into(),
        conn_facts,
    )]));
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "conn.prepare")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "statement.execute")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "entry.get")
            .unwrap()
            .status,
        "external"
    );

    // 6. Tree-sitter return inference
    let tree_source = r#"
        use tree_sitter::Parser;
        fn f(parser: &mut Parser) {
            let tree = parser.parse("x", None).unwrap();
            let root = tree.root_node(); root.kind();
            let child = root.child_by_field_name("body").unwrap(); child.kind();
        }
    "#;
    let tree_facts = ast::extract("src/probe_tree.rs", "rust", tree_source).unwrap();
    let tree_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tree.rs".into(),
        tree_facts,
    )]));
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "tree.root_node")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "root.kind")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "child.kind")
            .unwrap()
            .status,
        "external"
    );

    // 7. Parser-homonym negative (pulldown_cmark::Parser must NOT resolve to tree-sitter Node)
    let parser_homonym_source = r#"
        use pulldown_cmark::Parser;
        fn f(parser: &mut Parser) {
            let tree = parser.parse("x", None).unwrap();
            let root = tree.root_node(); root.kind();
        }
    "#;
    let parser_homonym_facts =
        ast::extract("src/probe_parser_neg.rs", "rust", parser_homonym_source).unwrap();
    let parser_homonym_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_parser_neg.rs".into(),
        parser_homonym_facts,
    )]));
    let root_kind = parser_homonym_graph
        .coverage
        .iter()
        .find(|c| c.expression == "root.kind")
        .expect("root.kind");
    assert_ne!(
        root_kind.status, "external",
        "pulldown_cmark::Parser must not resolve to tree-sitter Node: {root_kind:#?}"
    );

    // 8. While-let shadow followed by outer use (loop binding must not leak outside loop)
    let while_let_source = r#"
        struct Before; impl Before { fn finish(&self) {} }
        struct After; impl After { fn finish(&self) {} }
        fn f(option: Option<After>) {
            let entry = Before {};
            while let Some(entry) = option {
                entry.finish();
                break;
            }
            entry.finish();
        }
    "#;
    let while_let_facts = ast::extract("src/probe_while_let.rs", "rust", while_let_source).unwrap();
    let while_let_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_while_let.rs".into(),
        while_let_facts,
    )]));
    let finishes: Vec<_> = while_let_graph
        .coverage
        .iter()
        .filter(|c| c.expression == "entry.finish")
        .collect();
    assert_eq!(finishes.len(), 2, "expected 2 entry.finish calls");
    assert!(
        finishes[0]
            .evidence
            .contains("src.probe_while_let.After.finish"),
        "inner call must resolve to After: {:?}",
        finishes[0]
    );
    assert!(
        finishes[1]
            .evidence
            .contains("src.probe_while_let.Before.finish"),
        "post-loop call must resolve to Before: {:?}",
        finishes[1]
    );

    // 9. Tuple factory shadowed by local closure (must not link to top-level function's return type)
    let tuple_shadow_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn finish(&self) {} }
        fn make() -> (A, A) { (A {}, A {}) }
        fn f() {
            let make = || (B {}, B {});
            let (entry, _) = make();
            entry.finish();
        }
    "#;
    let tuple_shadow_facts =
        ast::extract("src/probe_tup_shadow.rs", "rust", tuple_shadow_source).unwrap();
    let tuple_shadow_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_shadow.rs".into(),
        tuple_shadow_facts,
    )]));
    let shadow_finish = tuple_shadow_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.finish")
        .expect("entry.finish");
    assert!(
        !shadow_finish
            .evidence
            .contains("src.probe_tup_shadow.A.finish"),
        "shadowed make() must NOT link to top-level A.finish: {shadow_finish:#?}"
    );

    // 9b. Nested function declaration shadows file-level function tuple factory
    let nested_fn_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn finish(&self) {} }
        fn make() -> (A, A) { (A {}, A {}) }
        fn f() {
            fn make() -> (B, B) { (B {}, B {}) }
            let (entry, _) = make();
            entry.finish();
        }
    "#;
    let nested_fn_facts = ast::extract("src/probe_nested_fn.rs", "rust", nested_fn_source).unwrap();
    let nested_fn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_nested_fn.rs".into(),
        nested_fn_facts,
    )]));
    let nested_finish = nested_fn_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.finish")
        .expect("entry.finish");
    assert!(
        !nested_finish
            .evidence
            .contains("src.probe_nested_fn.A.finish"),
        "nested fn make() must NOT link to top-level A.finish: {nested_finish:#?}"
    );
    assert!(
        nested_finish
            .evidence
            .contains("src.probe_nested_fn.B.finish"),
        "nested fn make() must link to inner B.finish: {nested_finish:#?}"
    );

    // 10. Local query_map callback (must not infer external rusqlite::Row)
    let local_qm_source = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Local; impl Local { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(val: Local) {
            val.query_map(|entry| entry.get(0));
        }
    "#;
    let local_qm_facts = ast::extract("src/probe_local_qm.rs", "rust", local_qm_source).unwrap();
    let local_qm_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_local_qm.rs".into(),
        local_qm_facts,
    )]));
    let local_entry_get = local_qm_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.get")
        .expect("entry.get");
    assert_ne!(
        local_entry_get.status, "external",
        "local query_map must NOT infer external rusqlite::Row: {local_entry_get:#?}"
    );

    // 10b. Local struct Connection query_map callback (must not infer external rusqlite::Row)
    let local_conn_source = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Connection;
        impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(db: Connection) {
            db.query_map(|entry| entry.get(0));
        }
    "#;
    let local_conn_facts =
        ast::extract("src/probe_local_conn.rs", "rust", local_conn_source).unwrap();
    let local_conn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_local_conn.rs".into(),
        local_conn_facts,
    )]));
    let conn_entry_get = local_conn_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.get")
        .expect("entry.get");
    assert_ne!(
        conn_entry_get.status, "external",
        "local struct Connection must NOT infer external rusqlite::Row: {conn_entry_get:#?}"
    );
    assert!(
        !conn_entry_get.evidence.contains("rusqlite::Row"),
        "evidence must NOT mention rusqlite::Row: {conn_entry_get:#?}"
    );

    // 11. Disabled Cargo pack in .forge/frameworks/rust-cargo.toml must not supply return policy
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!("forge-test-disabled-pack-{}", nonce));
    std::fs::create_dir_all(temp_root.join(".forge/frameworks")).unwrap();
    std::fs::write(
        temp_root.join(".forge/frameworks/rust-cargo.toml"),
        "enabled = false\n",
    )
    .unwrap();
    let r3_source = r#"
        use rusqlite::Connection;
        fn run(conn: Connection) {
            let mut statement = conn.prepare("SELECT 1").unwrap();
            statement.execute([]);
        }
    "#;
    let r3_facts = ast::extract("src/lib.rs", "rust", r3_source).unwrap();
    let r3_graph = linker::link_with_root(
        &std::collections::BTreeMap::from([("src/lib.rs".into(), r3_facts)]),
        None,
        Some(&temp_root),
    );
    let row = r3_graph
        .coverage
        .iter()
        .find(|r| r.expression == "statement.execute")
        .expect("statement.execute");
    assert!(
        !row.evidence.contains("rusqlite::Statement"),
        "disabled rust-cargo pack must NOT infer rusqlite::Statement: {row:#?}"
    );
    let _ = std::fs::remove_dir_all(&temp_root);

    // 11b. Disabled Cargo pack with whitespace (enabled  =  false) and qualified without import
    for (label, manifest, source) in [
        ("whitespace", "enabled  =  false\n", "use rusqlite::Connection;\nfn run(conn: Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }"),
        ("no-import", "enabled = false\n", "fn run(conn: rusqlite::Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }"),
    ] {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let temp_root = std::env::temp_dir().join(format!("forge-test-disabled-{}-{}", label, nonce));
        std::fs::create_dir_all(temp_root.join(".forge/frameworks")).unwrap();
        std::fs::write(temp_root.join(".forge/frameworks/rust-cargo.toml"), manifest).unwrap();
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let graph = linker::link_with_root(
            &std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]),
            None,
            Some(&temp_root),
        );
        let row = graph.coverage.iter().find(|r| r.expression == "s.execute").expect("s.execute");
        assert!(
            !row.evidence.contains("rusqlite::Statement"),
            "{label}: disabled rust-cargo pack must NOT infer rusqlite::Statement: {row:#?}"
        );
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    // 12. Lexical provenance: nested Connection or unrelated qualified connection with unrelated import
    for (label, source) in [
        ("nested-local", "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nfn f() { struct Connection; impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } } fn run(c: Connection) { c.query_map(|entry| entry.get(0)); } }"),
        ("qualified-local", "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nmod other { pub struct Connection; impl Connection { pub fn query_map<F: Fn(super::LocalItem)>(&self, f: F) { f(super::LocalItem {}); } } }\nfn f(c: other::Connection) { c.query_map(|entry| entry.get(0)); }"),
    ] {
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == "entry.get").expect("entry.get");
        assert_ne!(row.status, "external", "{label}: entry.get must not be external: {row:#?}");
        assert!(!row.evidence.contains("rusqlite::Row"), "{label}: evidence must not mention rusqlite::Row: {row:#?}");
    }

    // Contract revision 2: effective class hierarchy return policy preservation (LS001-T5-R3)
    {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("seam-contract-r2-{}-{}", std::process::id(), nonce));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".forge/frameworks")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='seam-r2'\nversion='0.1.0'\n[dependencies]\nrusqlite='0.31'\n",
        )
        .unwrap();
        let manifest = r#"enabled = true
replace = true
[[class_hierarchy_receivers]]
bases = ['rusqlite::Connection', 'Connection']
provider = 'rusqlite'
members = ['prepare', 'prepare_cached']
[class_hierarchy_receivers.returns]
prepare_cached = { kind = 'receiver', base = 'rusqlite::Row' }
[[class_hierarchy_receivers]]
bases = ['rusqlite::Row', 'Row']
provider = 'rusqlite'
members = ['get']
"#;
        std::fs::write(root.join(".forge/frameworks/rust-cargo.toml"), manifest).unwrap();
        let source = "use rusqlite::Connection;\nfn f(conn: Connection) { let value = conn.prepare_cached(\"SELECT 1\").unwrap(); value.get(0); }";
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let all = std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]);
        let graph = linker::link_with_root(&all, None, Some(&root));
        let row = graph
            .coverage
            .iter()
            .find(|r| r.expression == "value.get")
            .unwrap();
        assert!(
            row.evidence.contains("rusqlite::Row"),
            "effective return policy for prepare_cached -> Row must resolve on value.get: {row:#?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_chained_computed_receivers_and_compat() {
    // Task 6 Seam Contract: Chained computed receiver and workspace compat resolution
    {
        for (owner, target, import) in [
            (
                "crates/ordinary/src/probe.rs",
                "crates/ordinary/src/core.rs",
                "crate::core::work",
            ),
            (
                "crates/ordinary/src/probe.rs",
                "crates/ordinary/src/engine.rs",
                "crate::engine::work",
            ),
            ("examples/probe.rs", "examples/core.rs", "crate::core::work"),
            (
                "crates/forge-index/src/probe.rs",
                "crates/forge-core/src/models.rs",
                "forge_core::models::work",
            ),
            (
                "crates/forge-index/src/probe.rs",
                "crates/forge-index/src/ast.rs",
                "crate::ast::work",
            ),
            (
                "crates/forge-tasks/src/probe.rs",
                "crates/forge-tasks/src/store.rs",
                "crate::store::work",
            ),
            (
                "crates/ordinary/src/probe.rs",
                "crates/ordinary/src/db/tasks_store.rs",
                "crate::db::tasks_store::work",
            ),
        ] {
            let source = format!("use {import};\nfn probe() {{ work(); }}\n");
            let workspace = super::support::Workspace::new();
            workspace.write(owner, &source);
            workspace.write(target, "pub fn work() {}\n");
            let files = std::collections::BTreeMap::from([
                (owner.into(), ast::extract(owner, "rust", &source).unwrap()),
                (
                    target.into(),
                    ast::extract(target, "rust", "pub fn work() {}\n").unwrap(),
                ),
            ]);
            let graph = linker::link_with_root(&files, None, Some(&workspace.0));
            let caller = files[owner]
                .nodes
                .iter()
                .find(|node| node.name == "probe")
                .unwrap();
            let callee = files[target]
                .nodes
                .iter()
                .find(|node| node.name == "work")
                .unwrap();
            assert!(
                graph.edges.iter().any(|edge| edge.kind == "calls"
                    && edge.src == caller.id
                    && edge.dst == callee.id),
                "{owner}: {import}: {:#?}",
                graph.coverage
            );
        }
        let source = r#"
            use std::process::Command;
            use serde_json::Value;
            use rusqlite::{Connection, Statement, Row};
            type Syntax<'a> = tree_sitter::Node<'a>;
            fn probe_chained(opt: Option<&str>) {
                let s = opt.unwrap();
                let is_emp = s.trim().is_empty();
                opt.and_then(|s| Some(s)).unwrap().trim().is_empty();
            }
            fn probe_external(cmd: &mut Command, json: &Value) {
                cmd.args(["x"]).output();
                cmd.arg("x").output();
                json.get("key").unwrap().as_str().unwrap().trim().is_empty();
                json.as_array().unwrap().is_empty();
            }
            fn probe_references(conn: &Connection, stmt: &Statement, row: &Row, node: &Syntax<'_>) {
                conn.execute("", []); stmt.column_count(); row.get(0); node.kind();
            }
        "#;
        let facts = ast::extract("src/probe_task6_chain.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "src/probe_task6_chain.rs".into(),
            facts,
        )]));
        let is_empty_ref = graph
            .coverage
            .iter()
            .find(|c| c.expression.ends_with("is_empty"))
            .expect("is_empty coverage entry");
        assert_eq!(
            is_empty_ref.status, "external",
            "Task 6 Seam Contract: chained s.trim().is_empty() must classify as external Rust standard library built-in: {is_empty_ref:#?}"
        );
        assert!(
            is_empty_ref.evidence.contains("Rust standard library"),
            "evidence must confirm Rust standard library built-in: {is_empty_ref:#?}"
        );
        for expression in [
            "opt.and_then(|s| Some(s)).unwrap().trim().is_empty",
            "cmd.args([\"x\"]).output",
            "cmd.arg(\"x\").output",
            "json.get(\"key\").unwrap().as_str().unwrap().trim().is_empty",
            "json.as_array().unwrap().is_empty",
            "conn.execute",
            "stmt.column_count",
            "row.get",
            "node.kind",
        ] {
            let entry = graph
                .coverage
                .iter()
                .find(|c| c.expression == expression)
                .unwrap_or_else(|| panic!("missing {expression}: {:#?}", graph.coverage));
            assert_eq!(entry.status, "external", "{entry:#?}");
        }
        let files = [
            (
                "crates/ordinary/src/lib.rs",
                "mod inner; pub(crate) use crate::inner::work;",
            ),
            (
                "crates/ordinary/src/inner.rs",
                "pub(crate) fn work() {}",
            ),
            (
                "crates/ordinary/src/probe.rs",
                "use crate::work; fn root_probe() { work(); }",
            ),
            (
                "tests/probe.rs",
                "mod common; use crate::common::Workspace; fn probe() { let w = Workspace::new(); w.write(); }",
            ),
            (
                "tests/common/mod.rs",
                "mod workspace;\npub use workspace::Workspace;\n",
            ),
            (
                "tests/common/workspace.rs",
                "pub struct Workspace;\nimpl Workspace {\n pub fn new() -> Self { Self }\n pub fn write(&self) {}\n}\n",
            ),
            (
                "src/lang/mod.rs",
                "pub use tree_sitter::Node as Syntax;\npub fn helper() {}\nmod sub;\n",
            ),
            (
                "src/lang/sub/mod.rs",
                "use super::*;\nmod leaf;\n",
            ),
            (
                "src/lang/sub/leaf.rs",
                "use super::*;\nfn run(s: &Syntax) { s.kind(); helper(); }\n",
            ),
            (
                "src/parallel/mod.rs",
                "use rayon::prelude::*;\npub fn par_work() {}\nmod child;\n",
            ),
            (
                "src/parallel/child.rs",
                "use super::*;\nfn check() { par_work(); }\n",
            ),
            (
                "src/chains.rs",
                "use rusqlite::Connection;\nfn test_db(conn: &Connection) {\n    let mut stmt = conn.prepare(\"SELECT 1\").unwrap();\n    let mut rows = stmt.query([]).unwrap();\n    let row = rows.next().unwrap().unwrap();\n    let _val: i64 = row.get(0).unwrap();\n}\nfn test_syntax(node: &tree_sitter::Node<'_>) {\n    let child = node.child_by_field_name(\"test\").unwrap();\n    let named = child.named_child(0).unwrap();\n    let _ = named.kind();\n}\n",
            ),
            (
                "src/fields.rs",
                "struct Store { conn: rusqlite::Connection } impl Store { fn run(&self) { let stmt = self.conn.prepare_cached(\"SELECT 1\").unwrap(); stmt.column_count(); } } fn siblings(node: &tree_sitter::Node<'_>) { node.parent().unwrap().kind(); node.next_sibling().unwrap().kind(); node.prev_sibling().unwrap().kind(); node.next_named_sibling().unwrap().kind(); node.prev_named_sibling().unwrap().kind(); let mut cursor = node.walk(); node.children(&mut cursor).next().unwrap().kind(); node.named_children(&mut cursor).next().unwrap().kind(); }",
            ),
            (
                "crates/left/src/lib.rs",
                "mod inner;\npub(crate) use inner::work;\n",
            ),
            (
                "crates/left/src/inner.rs",
                "pub fn work() {}\n",
            ),
            (
                "crates/right/src/lib.rs",
                "use left::work;\nfn probe() { work(); }\n",
            ),
            (
                "src/opt_and_then.rs",
                "struct A;\nimpl A { fn finish(&self) {} }\nstruct B;\nimpl B { fn finish(&self) {} }\nfn probe(opt: Option<A>) { let result = opt.and_then(|_| Some(B)).unwrap(); result.finish(); }\n",
            ),
        ];
        let workspace = super::support::Workspace::new();
        for (path, source) in &files {
            workspace.write(path, source);
        }
        let graph = linker::link_with_root(
            &files
                .into_iter()
                .map(|(path, source)| (path.into(), ast::extract(path, "rust", source).unwrap()))
                .collect(),
            None,
            Some(&workspace.0),
        );
        for expression in ["Workspace::new", "w.write", "helper", "par_work", "work"] {
            let entry = graph
                .coverage
                .iter()
                .find(|c| c.expression == expression)
                .unwrap_or_else(|| panic!("missing {expression}: {:#?}", graph.coverage));
            assert_eq!(
                entry.status, "resolved",
                "{entry:#?}; {:#?}",
                graph.coverage
            );
        }
        for expression in [
            "s.kind",
            "conn.prepare(\"SELECT 1\").unwrap",
            "stmt.query([]).unwrap",
            "rows.next().unwrap().unwrap",
            "row.get(0).unwrap",
            "node.child_by_field_name(\"test\").unwrap",
            "child.named_child(0).unwrap",
            "named.kind",
            "self.conn.prepare_cached(\"SELECT 1\").unwrap",
            "stmt.column_count",
            "node.parent().unwrap().kind",
            "node.next_sibling().unwrap().kind",
            "node.prev_sibling().unwrap().kind",
            "node.next_named_sibling().unwrap().kind",
            "node.prev_named_sibling().unwrap().kind",
            "node.children(&mut cursor).next().unwrap().kind",
            "node.named_children(&mut cursor).next().unwrap().kind",
        ] {
            let entry = graph
                .coverage
                .iter()
                .find(|c| c.expression == expression)
                .unwrap_or_else(|| panic!("missing {expression}: {:#?}", graph.coverage));
            assert_eq!(entry.status, "external", "{entry:#?}");
        }
        let cross_crate_work = graph
            .coverage
            .iter()
            .find(|c| c.path == "crates/right/src/lib.rs" && c.expression == "work")
            .expect("right crate work call");
        assert_ne!(
            cross_crate_work.status, "resolved",
            "pub(crate) re-export in left crate must not resolve in right crate: {cross_crate_work:#?}"
        );
        let and_then_finish = graph
            .coverage
            .iter()
            .find(|c| c.path == "src/opt_and_then.rs" && c.expression == "result.finish")
            .expect("and_then finish call");
        assert_ne!(
            and_then_finish.status, "resolved",
            "type-changing and_then must not resolve to input type method: {and_then_finish:#?}"
        );
        for (label, expression, resolved) in [
            (
                "identity",
                "opt.and_then(|value| Some(value)).unwrap()",
                true,
            ),
            ("changed", "opt.and_then(|_| Some(B)).unwrap()", false),
            ("unknown", "opt.and_then(|_| opaque()).unwrap()", false),
            ("callback", "opt.and_then(callback).unwrap()", false),
            (
                "reassigned",
                "{ let mut value = opt.and_then(|v| Some(v)).unwrap(); value = opaque(); value }",
                false,
            ),
        ] {
            let source = format!("struct A; impl A {{ fn finish(&self) {{}} }} struct B; impl B {{ fn finish(&self) {{}} }} fn probe(opt: Option<A>) {{ let result = {expression}; result.finish(); }}");
            let facts = ast::extract("callbacks.rs", "rust", &source).unwrap();
            let caller = facts
                .nodes
                .iter()
                .find(|node| node.name == "probe")
                .unwrap();
            let target = facts
                .nodes
                .iter()
                .find(|node| node.qualname.ends_with(".A.finish"))
                .unwrap();
            let graph = linker::link(&std::collections::BTreeMap::from([(
                "callbacks.rs".into(),
                facts.clone(),
            )]));
            let calls: Vec<_> = graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.kind == "calls" && edge.src == caller.id && edge.dst == target.id
                })
                .collect();
            assert_eq!(
                calls.len(),
                usize::from(resolved),
                "{label}: {:#?}",
                graph.coverage
            );
        }

        for (policy, manifest, expected) in [
            ("default", None, Some("rusqlite::Row")),
            ("disabled", Some("enabled = false\n".to_owned()), None),
            ("no-return", Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['rusqlite::Rows']\nprovider = 'rusqlite'\nmembers = ['next']\n".to_owned()), None),
            ("remapped", Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['rusqlite::Rows']\nprovider = 'rusqlite'\nmembers = ['next']\n[class_hierarchy_receivers.returns]\nnext = { kind = 'receiver', base = 'tree_sitter::Node' }\n[[class_hierarchy_receivers]]\nbases = ['tree_sitter::Node']\nprovider = 'tree-sitter'\nmembers = ['kind']\n".to_owned()), Some("tree_sitter::Node")),
        ] {
            let workspace = super::support::Workspace::new();
            workspace.write("Cargo.toml", "[package]\nname='row-policy'\nversion='0.1.0'\n[dependencies]\nrusqlite='0.31'\ntree-sitter='0.22'\n");
            if let Some(manifest) = manifest { workspace.write(".forge/frameworks/rust-cargo.toml", &manifest); }
            workspace.write("src/lib.rs", "use rusqlite::{Rows, Row};\nfn assigned(rows: &mut Rows<'_>) { let row = rows.next().unwrap().unwrap(); row.get(0); row.kind(); }\nfn computed(rows: &mut Rows<'_>) { rows.next().unwrap().unwrap().get(0); rows.next().unwrap().unwrap().kind(); }\nfn generic(row: &Row) { let value = row.get(0).unwrap(); value.kind(); }\n");
            let db = workspace.build();
            for expression in ["row.get", "rows.next().unwrap().unwrap().get"] {
                let evidence: String = db.query_row("SELECT v.evidence FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE x.expression=?1", [expression], |row| row.get(0)).unwrap();
                assert_eq!(evidence.contains("rusqlite::Row"), expected == Some("rusqlite::Row"), "{policy}: {expression}: {evidence}");
            }
            if expected == Some("tree_sitter::Node") {
                let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression='row.kind'", [], |row| row.get(0)).unwrap();
                assert_eq!(status, "external", "{policy}");
            }
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression='value.kind'", [], |row| row.get(0)).unwrap();
            assert_eq!(status, "unresolved", "{policy}: generic Row.get payload must remain unknown");
        }
        for (visibility, outer_allowed, root_allowed, external_allowed) in [
            ("pub", true, true, true),
            ("pub(crate)", true, true, false),
            ("pub(super)", true, false, false),
            ("pub(in crate::outer)", true, false, false),
            ("pub(self)", false, false, false),
        ] {
            let workspace = super::support::Workspace::new();
            workspace.write(
                "Cargo.toml",
                "[workspace]\nmembers=['crates/left','crates/right']\n",
            );
            workspace.write(
                "crates/left/Cargo.toml",
                "[package]\nname='left'\nversion='0.1.0'\n",
            );
            workspace.write(
                "crates/right/Cargo.toml",
                "[package]\nname='right'\nversion='0.1.0'\n[dependencies]\nleft={path='../left'}\n",
            );
            workspace.write(
                "crates/left/src/lib.rs",
                "pub mod outer;\nuse crate::outer::api::work;\nfn root_probe(){ work(); }\n",
            );
            workspace.write(
                "crates/left/src/outer/mod.rs",
                "pub mod api;\npub mod inner;\nuse self::api::work;\nfn allowed(){ work(); }\n",
            );
            workspace.write("crates/left/src/outer/inner.rs", "pub fn work() {}\n");
            workspace.write(
                "crates/left/src/outer/api.rs",
                &format!("{visibility} use crate::outer::inner::work;\nmod leaf;\n"),
            );
            workspace.write(
                "crates/left/src/outer/api/leaf.rs",
                "use super::work;\nfn nested(){ work(); }\n",
            );
            workspace.write(
                "crates/right/src/lib.rs",
                "use left::outer::api::work;\nfn foreign(){ work(); }\n",
            );
            let db = workspace.build();
            for (caller, allowed) in [
                ("allowed", outer_allowed),
                ("root_probe", root_allowed),
                ("foreign", external_allowed),
                ("nested", true),
            ] {
                let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name=?1 AND d.name='work'", [caller], |row| row.get(0)).unwrap();
                assert_eq!(count, i64::from(allowed), "{visibility}: {caller}");
            }
        }
        for visibility in ["pub", "pub(crate)"] {
            for (case, declarations, providers, foreign_import, foreign_call) in [
                (
                    "file-item",
                    "VIS mod inner;",
                    vec![("inner.rs", "pub fn work() {}")],
                    "use left::inner::work;",
                    "work()",
                ),
                (
                    "file-module",
                    "VIS mod inner;",
                    vec![("inner.rs", "pub fn work() {}")],
                    "use left::inner;",
                    "inner::work()",
                ),
                (
                    "inline-item",
                    "VIS mod inner { pub fn work() {} }",
                    vec![],
                    "use left::inner::work;",
                    "work()",
                ),
                (
                    "inline-module",
                    "VIS mod inner { pub fn work() {} }",
                    vec![],
                    "use left::inner;",
                    "inner::work()",
                ),
                (
                    "file-reexport",
                    "pub mod inner; VIS mod api;",
                    vec![
                        ("inner.rs", "pub fn work() {}"),
                        ("api.rs", "pub use crate::inner::work;"),
                    ],
                    "use left::api::work;",
                    "work()",
                ),
                (
                    "ancestor-module",
                    "VIS mod outer;",
                    vec![
                        ("outer.rs", "pub mod inner;"),
                        ("outer/inner.rs", "pub fn work() {}"),
                    ],
                    "use left::outer::inner::work;",
                    "work()",
                ),
                (
                    "public-facade",
                    "VIS mod inner; pub use crate::inner::work;",
                    vec![("inner.rs", "pub fn work() {}")],
                    "use left::work;",
                    "work()",
                ),
            ] {
                let workspace = super::support::Workspace::new();
                workspace.write(
                    "Cargo.toml",
                    "[workspace]\nmembers=['crates/left','crates/right']\n",
                );
                workspace.write(
                    "crates/left/Cargo.toml",
                    "[package]\nname='left'\nversion='0.1.0'\n",
                );
                workspace.write("crates/right/Cargo.toml", "[package]\nname='right'\nversion='0.1.0'\n[dependencies]\nleft={path='../left'}\n");
                let local_call = match case {
                    "file-reexport" => "api::work()",
                    "ancestor-module" => "work()",
                    "public-facade" => "work()",
                    _ => "inner::work()",
                };
                workspace.write(
                    "crates/left/src/lib.rs",
                    &format!(
                        "{}\nfn local() {{ {local_call}; }}\n",
                        declarations.replace("VIS", visibility)
                            + if case == "ancestor-module" {
                                " use crate::outer::inner::work;"
                            } else {
                                ""
                            }
                    ),
                );
                for (path, source) in providers {
                    workspace.write(format!("crates/left/src/{path}"), source);
                }
                workspace.write(
                    "crates/right/src/lib.rs",
                    &format!("{foreign_import}\nfn foreign() {{ {foreign_call}; }}\n"),
                );
                let db = workspace.build();
                for (caller, expected) in [
                    ("local", true),
                    ("foreign", visibility == "pub" || case == "public-facade"),
                ] {
                    let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name=?1 AND d.name='work'", [caller], |row| row.get(0)).unwrap();
                    assert_eq!(count, i64::from(expected), "{visibility} {case}: {caller}");
                }
            }
        }
        {
            let workspace = super::support::Workspace::new();
            let unrelated = super::support::Workspace::new();
            workspace.write(
                "Cargo.toml",
                "[workspace]\nmembers=['crates/left','crates/right']\n",
            );
            workspace.write(
                "crates/left/Cargo.toml",
                "[package]\nname='left'\nversion='0.1.0'\n",
            );
            workspace.write(
                "crates/right/Cargo.toml",
                "[package]\nname='right'\nversion='0.1.0'\n[dependencies]\nleft={path='../left'}\n",
            );
            workspace.write("crates/left/src/lib.rs", "pub mod models; pub use models::*; pub fn work() {}\n");
            workspace.write("crates/left/src/models.rs", "pub struct Node { pub visible: String, pub(crate) internal: String, private: String } impl Node { pub fn new() -> Self { Self { visible: String::new(), internal: String::new(), private: String::new() } } pub fn finish(&self) {} }\n");
            workspace.write(
                "crates/right/src/lib.rs",
                "use left::*;\nfn probe(){ work(); let node = Node::new(); node.finish(); }\nfn borrowed(node: &Node) { node.finish(); let _ = &node.visible; let _ = &node.internal; let _ = &node.private; }\n",
            );
            for (index, cwd) in [&workspace.0, &unrelated.0].into_iter().enumerate() {
                let db_path = workspace.0.join(format!(".forge/cwd-{index}.sqlite"));
                let output =
                    std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                        .arg("--root")
                        .arg(&workspace.0)
                        .arg("--db")
                        .arg(&db_path)
                        .args(["--format", "json", "build"])
                        .current_dir(cwd)
                        .output()
                        .unwrap();
                assert!(
                    output.status.success(),
                    "cwd {cwd:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let db = rusqlite::Connection::open(db_path).unwrap();
                let calls: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='probe' AND d.name='work'", [], |row| row.get(0)).unwrap();
                assert_eq!(calls, 1, "explicit-root import resolution at cwd {cwd:?}");
                let methods: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='probe' AND d.name='finish'", [], |row| row.get(0)).unwrap();
                assert_eq!(methods, 1, "cross-crate imported Node receiver at cwd {cwd:?}");
                let borrowed: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='borrowed' AND d.name='finish'", [], |row| row.get(0)).unwrap();
                assert_eq!(borrowed, 1, "cross-crate glob parameter identity at cwd {cwd:?}");
                for expression in ["node.visible", "node.internal", "node.private"] {
                    let reads: i64 = db.query_row("SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='crates/right/src/lib.rs' AND x.expression=?1", [expression], |row| row.get(0)).unwrap();
                    assert_eq!(reads, 0, "ordinary field reads must not expand reference inventory at cwd {cwd:?}: {expression}");
                }
            }
        }
    }

    assert!(ast::parser("sql", "schema.sql").is_err());
    let mut public_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::parse_file(
        &contextunity_forge_mcp::engine::languages::sql::SQL,
        "public.sql",
        "CREATE TABLE public_test (id INT)",
        "public",
        &mut public_facts,
    )
    .unwrap();
    assert!(public_facts
        .nodes
        .iter()
        .any(|node| node.name == "public_test"));
    for (sql, name, relation) in [
        (
            "SELECT * FROM public.orders",
            "public.orders",
            "queries_table",
        ),
        (
            "SELECT * FROM orders, customers",
            "customers",
            "queries_table",
        ),
        (
            "BEGIN; PRAGMA x=1; SELECT * FROM orders",
            "orders",
            "queries_table",
        ),
        (
            "SET search_path TO shop; CREATE TABLE batch_table (id INT)",
            "batch_table",
            "defines_table",
        ),
        (
            "INSERT OR REPLACE INTO orders VALUES (1)",
            "orders",
            "mutates_table",
        ),
        ("UPDATE orders SET id = 1", "orders", "mutates_table"),
        ("COMMIT; SELECT * FROM orders", "orders", "queries_table"),
        ("ROLLBACK; UPDATE orders AS o SET id = 1", "orders", "mutates_table"),
        ("UPDATE ONLY orders o SET id = 1", "orders", "mutates_table"),
        ("WITH source AS (SELECT * FROM customers) UPDATE ONLY (orders) AS o SET id=1", "orders", "mutates_table"),
        ("SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i", "inventory", "queries_table"),
        ("SELECT * FROM orders o, inventory i JOIN customers c ON c.id=i.id", "customers", "queries_table"),
        ("WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1", "orders", "queries_table"),
        ("COMMIT; SELECT * FROM orders WHERE note = $$semi;colon$$", "orders", "queries_table"),
        ("SET x='semi;SELECT fake'; /* ; UPDATE ignored */ SELECT * FROM orders", "orders", "queries_table"),
        ("BEGIN; -- ; UPDATE ignored\nWITH rows AS (SELECT * FROM orders) SELECT * FROM rows", "orders", "queries_table"),
        ("SET x=$tag$; UPDATE ignored$tag$; SELECT * FROM orders", "orders", "queries_table"),
        ("DELETE FROM orders", "orders", "mutates_table"),
        (
            "CREATE TABLE IF NOT EXISTS orders (id INT)",
            "orders",
            "defines_table",
        ),
    ] {
        assert!(
            contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql)
                .contains(&(name.to_owned(), relation))
        );
    }

    for (sql, expected) in [
        ("WITH temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec!["temporary"]),
        ("WITH RECURSIVE temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec![]),
        ("WITH temporary(id) AS (SELECT * FROM orders) SELECT * FROM public.temporary", vec!["orders", "public.temporary"]),
        ("WITH first_cte AS (SELECT * FROM orders), second_cte(id) AS (SELECT * FROM first_cte) SELECT * FROM second_cte", vec!["orders"]),
    ] {
        let actual = contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql);
        assert_eq!(actual.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), expected, "{sql}");
    }

    // Exercise the real literal producers, including prose that previously
    // matched SQL keywords inside unrelated words.
    let mut embedded = vec![(
        "embedded.rs",
        "rust",
        r##"fn query() { let _ = r#"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows"#; }
fn prose() { let _ = "information was updated and transformed"; }"##,
    )];
    #[cfg(feature = "lang-python")]
    embedded.push((
        "embedded.py",
        "python",
        "def query():\n    return \"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows\"\ndef prose():\n    '''information was updated and transformed'''\n",
    ));
    #[cfg(feature = "lang-typescript")]
    embedded.push((
        "embedded.ts",
        "typescript",
        "function query() { return `SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows`; }\nfunction prose() { return 'information was updated and transformed'; }\n",
    ));
    for (path, language, source) in embedded {
        let facts = ast::extract(path, language, source).unwrap();
        let queries: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.kind == "queries_table")
            .collect();
        assert_eq!(queries.len(), 1, "{path}: {queries:?}");
        assert_eq!(queries[0].expression, "orders", "{path}");
        assert!(
            !facts.references.iter().any(|r| r.kind == "mutates_table"),
            "{path}"
        );
    }

    let workspace = super::support::Workspace::new();
    workspace.write("more_tables.sql", "CREATE TABLE inventory (id INTEGER); CREATE TABLE factory_orders (id INTEGER); CREATE TABLE temporary (id INTEGER);");
    workspace.write(
        "quoted_schema.sql",
        "CREATE TABLE \"quoted_table\" (id INTEGER);",
    );
    workspace.write(
        "case_schema.sql",
        "CREATE TABLE \"Foo\" (id INTEGER); CREATE TABLE \"foo\" (id INTEGER);",
    );
    workspace.write(
        "Cargo.toml",
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n[dependencies]\nsqlx = '0.8'\n",
    );
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE customers (id INTEGER);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE shop_override (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id) ON UPDATE CASCADE, PRIMARY KEY (id));\nCREATE TABLE a.same (id INTEGER); CREATE TABLE b.same (id INTEGER); CREATE TABLE \"x.y\" (id INT); CREATE TABLE x.y (id INT); CREATE TABLE SET (id INT); CREATE TABLE CASCADE (id INT);\nCREATE VIEW order_view AS SELECT * FROM orders;\n");
    workspace.write(
        "extra_schema.sql",
        "CREATE TABLE shop_default (id INTEGER);",
    );
    workspace.write(
        "migration.sql",
        "ALTER TABLE orders ADD COLUMN note TEXT;\n",
    );
    workspace.write("queries.rs", "#[derive(sqlx::FromRow)] struct OrderRow { id: i64 }\nfn load() { let _ = \"SELECT * FROM orders\"; }\nfn save() { let _ = \"INSERT INTO orders (id) VALUES (1)\"; }\nfn remove() { let _ = \"DELETE FROM orders WHERE id = 1\"; }\nfn customers() { let _ = \"SELECT * FROM customers\"; }\nfn macro_load() { let _ = sqlx::query!(\"SELECT * FROM orders\"); }\nfn typed_load() { let _ = sqlx::query_as!(OrderRow, \"SELECT * FROM orders\"); }\nfn inline_schema() { let _ = \"CREATE TABLE local_data (id INTEGER)\"; }\n");
    workspace.write("foreign.rs", "struct Imported;\nuse external::Imported;\nfn foreign() { let _ = sqlx::query_as!(Imported, \"SELECT * FROM orders\"); }\n");
    workspace.write("qualified.rs", "fn exact() { let _ = \"SELECT * FROM a.same\"; }\nfn ambiguous_short() { let _ = \"SELECT * FROM same\"; }\nfn batch() { let _ = \"BEGIN; PRAGMA x=1; SELECT * FROM orders, customers\"; }\nfn inline_create() { let _ = \"CREATE TABLE c.inline_qual (id INT)\"; }\nfn inline_query() { let _ = \"SELECT * FROM inline_qual\"; }\n");
    workspace.write("upsert.rs", "fn upsert() { let _ = \"INSERT INTO orders VALUES (1) ON CONFLICT DO UPDATE SET id=1\"; }\n");
    workspace.write("patterns.rs", "fn rollback_update() { let _ = \"ROLLBACK; UPDATE ONLY orders AS o SET id=1\"; }\nfn mixed_from() { let _ = \"SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i\"; }\nfn cte_query() { let _ = \"WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1\"; }\n");
    #[cfg(feature = "lang-python")]
    workspace.write("identifiers.py", "def quoted_orders():\n    return 'SELECT * FROM \"orders\"'\ndef unquoted_table():\n    return 'SELECT * FROM quoted_table'\ndef cte_columns():\n    return 'WITH temporary(id) AS (SELECT id FROM orders) SELECT * FROM temporary'\n");
    workspace.write("ambiguous.rs", "mod a { struct Shared; }\nmod b { struct Shared; }\nfn ambiguous() { let _ = sqlx::query_as!(Shared, \"SELECT * FROM orders\"); }\n");
    workspace.write("shadow.rs", "mod sqlx {}\nstruct Shadow;\nfn shadow() { let _ = sqlx::query_as!(Shadow, \"SELECT * FROM orders\"); }\n");
    workspace.write("other.rs", "struct Other;\nfn other() { let _ = custom::query_as!(Other, \"SELECT * FROM orders\"); }\n");
    workspace.write(
        "derive_only.rs",
        "#[derive(sqlx::FromRow)] struct DeriveOnly { id: i64 }\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("queries.py", "from django.db import models\nfrom sqlalchemy.orm import DeclarativeBase\ndef py_load():\n    return \"SELECT * FROM orders\"\n\nclass Base(DeclarativeBase):\n    pass\nclass Order(Base):\n    __tablename__ = 'orders'\n\nclass FalseSqlAlchemy:\n    __tablename__ = 'orders'\n\nclass Item(models.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("quoted.py", "def quoted():\n    return 'SELECT * FROM \"x.y\"'\ndef qualified():\n    return 'SELECT * FROM x.y'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("case_query.py", "def upper_case():\n    return 'SELECT * FROM \"Foo\"'\ndef lower_case():\n    return 'SELECT * FROM \"foo\"'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory.py", "from sqlalchemy.orm import declarative_base as make_base\nBase = make_base()\nclass FactoryOrder(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_qualified.py", "import sqlalchemy.orm as orm\nBase = orm.declarative_base()\nclass QualifiedFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_invalid.py", "from sqlalchemy.orm import declarative_base as make_base\nmake_base = object()\nBase = make_base()\nclass ReboundFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/other-tables.toml", "[[semantic_rules]]\nid = 'other-table'\nnamespace = 'other.tables'\nscope = 'project'\noperation = { kind = 'class_attribute_to_table', language = 'python', attribute = 'table_name', container = 'Config' }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/standalone-default.toml", "[[semantic_rules]]\nid = 'default'\nnamespace = 'other.default'\nscope = 'project'\noperation = { kind = 'class_default_table', language = 'python', bases = ['fixture.CustomBase'] }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/default.py",
        "# class Commented(CustomBase): pass\n\"\"\"class Quoted(CustomBase): pass\"\"\"\nfrom fixture import CustomBase\nclass Empty:\n    pass\nclass Default(CustomBase):\n    pass\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "other_model.py",
        "class Generic:\n    class Config:\n        table_name = 'orders'\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'shop'\nversion = '0.1'\ndependencies = ['django', 'sqlalchemy']\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/models.py",
        "from django.db import models as dbmodels\nclass Cart(dbmodels.Model):\n    pass\nclass Override(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\nclass NonModel:\n    class Meta:\n        db_table = 'orders'\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("shop/reassigned.py", "from django.db import models as dbmodels\ndbmodels = object()\nclass Reassigned(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/fake.py",
        "from local import models\nclass Fake(models.Model):\n    pass\n",
    );
    #[cfg(feature = "lang-typescript")]
    workspace.write(
        "queries.ts",
        "function ts_load() { return \"SELECT * FROM orders\"; }\n",
    );
    let db = workspace.build();
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted_orders", "orders", 1),
        ("unquoted_table", "quoted_table", 1),
        ("cte_columns", "temporary", 0),
        ("cte_columns", "orders", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("upper_case", "\"Foo\"", 1),
        ("upper_case", "\"foo\"", 0),
        ("lower_case", "\"foo\"", 1),
        ("lower_case", "\"Foo\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "quoted case: {source} -> {target}");
    }
    let fake_cte: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='cte_query' AND d.name='temporary' AND e.kind='queries_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_cte, 0,
        "CTE name must not be resolved to a physical table"
    );
    #[cfg(feature = "lang-python")]
    {
        let edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='FactoryOrder' AND d.name='factory_orders' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            edge, 1,
            "declarative_base factory must prove model ancestry"
        );
        let qualified: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='QualifiedFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(qualified, 1, "qualified imported factory must be resolved");
        let rebound: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='ReboundFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(rebound, 0, "rebound factory must fail closed");
    }
    let qualified: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('a.same','b.same')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(qualified, 2, "qualified tables retain distinct identities");
    let dotted: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('\"x.y\"','x.y')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dotted, 2, "quoted and qualified identities differ");
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted", "\"x.y\"", 1),
        ("quoted", "x.y", 0),
        ("qualified", "x.y", 1),
        ("qualified", "\"x.y\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let fake_dml: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name IN ('upsert','item') AND d.name IN ('SET','CASCADE') AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_dml, 0,
        "UPDATE clauses are not standalone table writes"
    );
    for (source, target, expected) in [
        ("exact", "a.same", 1),
        ("exact", "b.same", 0),
        ("ambiguous_short", "a.same", 0),
        ("ambiguous_short", "b.same", 0),
        ("batch", "customers", 1),
        ("batch", "orders", 1),
        ("inline_query", "c.inline_qual", 1),
        ("mixed_from", "inventory", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let update_edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='rollback_update' AND d.name='orders' AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(update_edge, 1, "batch UPDATE with alias and ONLY must link");
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind = 'table' AND name = 'orders'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1, "SQL schema tables must be indexed");
    let inline: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND name='local_data'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inline, 1, "inline CREATE must synthesize a table");
    let inline_link: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='inline_schema' AND d.name='local_data' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(inline_link, 1, "inline CREATE must link its owner");
    let model: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='OrderRow' AND d.name='orders' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        model, 1,
        "query_as must link its unambiguous local result type"
    );
    let invalid: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Shared','Imported','Shadow','Other','DeriveOnly') AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        invalid, 0,
        "ambiguous, imported, shadowed, unconfigured and derive-only identities must remain unresolved"
    );
    let mut expected = vec![
        ("order_view", "queries_table"),
        ("load", "queries_table"),
        ("save", "mutates_table"),
        ("remove", "mutates_table"),
        ("macro_load", "queries_table"),
        ("typed_load", "queries_table"),
        ("item", "references_table"),
    ];
    #[cfg(feature = "lang-python")]
    expected.push(("py_load", "queries_table"));
    #[cfg(feature = "lang-python")]
    expected.extend([
        ("Order", "defines_table"),
        ("Item", "defines_table"),
        ("Generic", "defines_table"),
    ]);
    #[cfg(feature = "lang-typescript")]
    expected.push(("ts_load", "queries_table"));
    for (source, kind) in expected {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='orders' AND e.kind=?2",
            [source, kind], |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1, "missing {kind} edge from {source}");
    }
    let columns: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname IN ('orders.id', 'item.order_id')", [], |row| row.get(0)).unwrap();
    assert_eq!(columns, 2);
    let altered: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname='orders.note' AND path_id=(SELECT path_id FROM path_dictionary WHERE path='migration.sql')", [], |row| row.get(0)).unwrap();
    assert_eq!(altered, 1, "ALTER TABLE must index the added column");
    #[cfg(feature = "lang-python")]
    {
        let defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Cart' AND d.name='shop_cart' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(defaults, 1, "Django default table should link");
        let other_default: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Default' AND d.name='shop_default' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            other_default, 1,
            "independent default-only manifest should link"
        );
        let empty_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='Empty' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            empty_defaults, 0,
            "class without bases must not define a table"
        );
        let false_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Fake' AND d.name='shop_fake' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(false_defaults, 0, "local models must not imply Django ORM");
        let override_edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Override' AND d.name='shop_override' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(override_edges, 0, "explicit Meta table overrides default");
        let invalid_models: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('NonModel','Reassigned','FalseSqlAlchemy') AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            invalid_models, 0,
            "unproven Django identities remain unresolved"
        );
    }
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .unwrap();
    let impact = contextunity_forge_mcp::db::traversal::traverse_with_options(
        &db,
        "schema.sql:table:orders",
        &contextunity_forge_mcp::db::traversal::TraversalOptions {
            depth: 1,
            inbound: true,
            mode: None,
            edge_types: None,
            page: &page,
        },
    )
    .unwrap();
    let reached = impact["nodes"]["items"].as_array().unwrap();
    assert!(
        reached.iter().any(|node| node["name"] == "order_view"),
        "SQL impact must include views: {impact}"
    );
    for name in ["load", "save", "remove", "item"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    #[cfg(feature = "lang-python")]
    for name in ["Order", "Item", "py_load"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    drop(db);
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id), PRIMARY KEY (id));\nCREATE VIEW order_view AS SELECT * FROM orders;\nCREATE TABLE customers (id INTEGER);\n");
    workspace.cold_parity("schema.sql");
    let delta = workspace.open();
    let cold = contextunity_forge_mcp::db::reader::open(
        &workspace.0.join(".forge/cold.sqlite"),
        &workspace.0,
    )
    .unwrap();
    let root = |conn: &rusqlite::Connection| -> String {
        conn.query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        root(&delta),
        root(&cold),
        "SQL cold and delta commitments must agree"
    );
    let without_sqlx = super::support::Workspace::new();
    without_sqlx.write("schema.sql", "CREATE TABLE orders (id INTEGER);\n");
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive.py", "from sqlalchemy.orm import DeclarativeBase\nclass Inactive(DeclarativeBase):\n    __tablename__ = 'orders'\n");
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive_factory.py", "from sqlalchemy.orm import declarative_base\nBase = declarative_base()\nclass InactiveFactory(Base):\n    __tablename__ = 'orders'\n");
    without_sqlx.write("query.rs", "struct NotActive;\nfn load() { let _ = sqlx::query_as!(NotActive, \"SELECT * FROM orders\"); }\n");
    let db = without_sqlx.build();
    let edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='NotActive' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(edges, 0, "undeclared SQLx must not activate its model rule");
    #[cfg(feature = "lang-python")]
    {
        let inactive: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Inactive','InactiveFactory') AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            inactive, 0,
            "undeclared SQLAlchemy must not activate its model rule"
        );
    }

    {
        let source = r#"
struct Item;
impl Item { fn finish(&self) {} }
struct Inner { nodes: Vec<Item> }
struct Facts { inner: Inner }
struct Root(std::path::PathBuf);
fn fields(facts: &Facts, root: &Root) {
    for node in facts.inner.nodes.as_slice().iter() { node.finish(); }
    let nodes = facts.inner.nodes.iter().filter(|_| true).collect();
    for collected in nodes { collected.finish(); }
    root.0.join("child");
}
"#;
        let facts = ast::extract("nested_fields.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "nested_fields.rs".into(),
            facts,
        )]));
        for (expression, expected) in [
            ("node.finish", "resolved"),
            ("collected.finish", "resolved"),
            ("root.0.join", "external"),
        ] {
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, expected, "{coverage:#?}");
        }
    }
    {
        let workspace = super::support::Workspace::new();
        workspace.write(
            "tests/common/mod.rs",
            "#[path = \"workspace.rs\"] mod workspace; pub use workspace::Workspace;",
        );
        workspace.write(
            "tests/common/workspace.rs",
            include_str!("../common/workspace.rs"),
        );
        workspace.write("tests/acdd/mod.rs", "pub(crate) mod support;");
        workspace.write("tests/acdd/support.rs", include_str!("../acdd/support.rs"));
        workspace.write("tests/helpers.rs", "mod common; mod acdd; use common::Workspace; use acdd::support::ScopedWorkspace; fn probe() { let workspace = Workspace::new(); workspace.write(\"a.rs\", \"\"); workspace.delta(&[\"a.rs\"]); workspace.build(); workspace.open(); let scoped = ScopedWorkspace::new(\"probe\"); scoped.write(\"a.rs\", \"\"); scoped.0.join(\"child\"); }");
        let db = workspace.build();
        for member in ["write", "delta", "build", "open"] {
            let expression = format!("workspace.{member}");
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/helpers.rs' AND x.expression=?1", [&expression], |row| row.get(0)).unwrap();
            assert_eq!(status, "resolved", "cross-file Self return: {expression}");
        }
        for (expression, expected) in [("scoped.write", "resolved"), ("scoped.0.join", "external")]
        {
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/helpers.rs' AND x.expression=?1", [expression], |row| row.get(0)).unwrap();
            assert_eq!(status, expected, "ScopedWorkspace: {expression}");
        }
    }

    {
        let members = [
            ("filter", "|_| true"),
            ("map", "|item| item"),
            ("filter_map", "|item| Some(item)"),
            ("flat_map", "|item| Some(item)"),
            ("inspect", "|_| {}"),
            ("fuse", ""),
            ("collect", ""),
            ("enumerate", ""),
            ("take", "1"),
            ("skip", "1"),
            ("for_each", "|_| {}"),
        ];
        let mut source = String::from("struct Item;\n");
        for (index, (member, arguments)) in members.iter().enumerate() {
            source.push_str(&format!("fn probe_{index}(items: Vec<Item>) {{ let stream = items.into_iter(); stream.{member}({arguments}); }}\n"));
        }
        source.push_str("fn guards(option: Option<Item>, result: Result<Item, ()>, items: Vec<Item>) { option.is_some_and(|_| true); option.is_none_or(|_| true); result.is_ok_and(|_| true); result.is_err_and(|_| true); let slice = items.as_slice(); slice.len(); slice.is_empty(); let stream = items.into_iter(); stream.made_up(); }\n");
        let facts = ast::extract("owned_iterators.rs", "rust", &source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "owned_iterators.rs".into(),
            facts,
        )]));
        for expression in members
            .iter()
            .map(|(member, _)| format!("stream.{member}"))
            .chain([
                "option.is_some_and".into(),
                "option.is_none_or".into(),
                "result.is_ok_and".into(),
                "result.is_err_and".into(),
                "slice.len".into(),
                "slice.is_empty".into(),
            ])
        {
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, "external", "{coverage:#?}");
        }
        let unknown = graph
            .coverage
            .iter()
            .find(|row| row.expression == "stream.made_up")
            .unwrap();
        assert_eq!(unknown.status, "unresolved", "{unknown:#?}");
    }

    {
        let workspace = super::support::Workspace::new();
        workspace.write(
            "src/lib.rs",
            "use tree_sitter::Node as Syntax; mod a; pub mod support; mod consumer; mod shadow;\n",
        );
        workspace.write("src/a/mod.rs", "use super::Syntax as Tree; mod b;\n");
        workspace.write(
            "src/a/b/mod.rs",
            "use super::Tree as Syntax; mod named; mod glob; mod shadow;\n",
        );
        for (file, import) in [("named", "use super::{Syntax};"), ("glob", "use super::*;")] {
            workspace.write(format!("src/a/b/{file}.rs"), &format!("{import} fn probe(node: Syntax<'_>) {{ node.kind(); node.child_by_field_name(\"body\"); node.walk(); }}"));
        }
        workspace.write(
            "src/a/b/shadow.rs",
            "use super::*; struct Syntax; fn probe(node: Syntax) { node.kind(); }",
        );
        workspace.write("src/support.rs", "pub struct Root(pub std::path::PathBuf); impl Root { pub fn write(&self) {} } pub struct Store; impl Store { pub fn sync(&mut self) {} } pub struct Milestone; impl Milestone { pub fn task_id(&self) {} } pub fn fixture() -> (Root, Store, Milestone) { panic!() }");
        workspace.write("src/consumer.rs", "use crate::support::fixture as setup; fn probe() { let (root, mut store, milestone) = setup(); root.write(); root.0.join(\"child\"); store.sync(); milestone.task_id(); }");
        workspace.write("src/shadow.rs", "use crate::support::fixture; fn negative() { let fixture = || ((), (), ()); let (root, store, milestone) = fixture(); root.write(); store.sync(); milestone.task_id(); }");
        let db = workspace.build();
        for (path, expression, expected) in [
            ("src/a/b/named.rs", "node.kind", "external"),
            ("src/a/b/named.rs", "node.child_by_field_name", "external"),
            ("src/a/b/named.rs", "node.walk", "external"),
            ("src/a/b/glob.rs", "node.kind", "external"),
            ("src/a/b/glob.rs", "node.walk", "external"),
            ("src/a/b/shadow.rs", "node.kind", "unresolved"),
            ("src/consumer.rs", "root.write", "resolved"),
            ("src/consumer.rs", "root.0.join", "external"),
            ("src/consumer.rs", "store.sync", "resolved"),
            ("src/consumer.rs", "milestone.task_id", "resolved"),
            ("src/shadow.rs", "root.write", "unresolved"),
        ] {
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path=?1 AND x.expression=?2", [path, expression], |row| row.get(0)).unwrap();
            assert_eq!(status, expected, "{path}: {expression}");
        }
        for method in ["write", "sync", "task_id"] {
            let calls: i64 = db.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target ON target.path_id=n.path_id WHERE owner.path='src/consumer.rs' AND target.path='src/support.rs' AND e.kind='calls' AND n.name=?1", [method], |row| row.get(0)).unwrap();
            assert_eq!(
                calls, 1,
                "imported tuple field must call canonical {method}"
            );
        }
        let shadow_calls: i64 = db.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target ON target.path_id=n.path_id WHERE owner.path='src/shadow.rs' AND target.path='src/support.rs' AND e.kind='calls'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            shadow_calls, 0,
            "shadowed factory must not acquire tuple payloads"
        );
    }
    {
        let source = "use rusqlite::{Connection, Statement}; struct Fake; impl Fake { fn query_row<F>(&self, callback: F) {} } fn probe(conn: &Connection, statement: &mut Statement, fake: Fake) { conn.query_row(\"sql\", [], |row| row.get(0)); conn.query_row::<String, _, _>(\"sql\", [], |typed| typed.get(0)); statement.query_map([], |r| r.get(0)); fake.query_row(|unproven| unproven.get(0)); }";
        let facts = ast::extract("query_callbacks.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "query_callbacks.rs".into(),
            facts,
        )]));
        for (expression, expected) in [
            ("row.get", "external"),
            ("typed.get", "external"),
            ("r.get", "external"),
            ("unproven.get", "unresolved"),
        ] {
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, expected, "{coverage:#?}");
        }
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_dynamic_receiver_hints_and_field_adapters() {
    {
        let source = r#"
struct Service;
impl Service { fn finish(&self) {} fn len(&self) {} }
struct Opaque;
fn build() -> Service { Service }
fn unknown() -> Opaque { Opaque }
fn proven() { build().finish(); build().len(); }
fn unproven(workspace: Opaque, root: Opaque, conn: Opaque, row: Opaque, client: Opaque, profile: Opaque, m: Opaque) {
    workspace.write(); root.join(); conn.query_row(); row.get(); client.call(); profile.id(); m.unwrap(); unknown().unwrap();
}
"#;
        let facts = ast::extract("dynamic_receivers.rs", "rust", source).unwrap();
        let targets: Vec<String> = facts
            .nodes
            .iter()
            .filter(|node| node.kind == "method" && matches!(node.name.as_str(), "finish" | "len"))
            .map(|node| node.id.clone())
            .collect();
        assert_eq!(targets.len(), 2);
        for expression in ["build().finish", "build().len"] {
            let reference = facts
                .references
                .iter()
                .find(|r| r.expression == expression)
                .unwrap();
            assert!(reference.receiver_hint.is_some(), "{reference:#?}");
        }
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "dynamic_receivers.rs".into(),
            facts,
        )]));
        for expression in ["build().finish", "build().len"] {
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, "resolved", "{coverage:#?}");
        }
        for expression in [
            "workspace.write",
            "root.join",
            "conn.query_row",
            "row.get",
            "client.call",
            "profile.id",
            "m.unwrap",
            "unknown().unwrap",
        ] {
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, "unresolved", "{coverage:#?}");
        }
        for target in targets {
            assert_eq!(
                graph
                    .edges
                    .iter()
                    .filter(|edge| edge.kind == "calls" && edge.dst == target)
                    .count(),
                1
            );
        }
    }

    {
        let fields = [
            "ordinary",
            "iter",
            "next",
            "get",
            "unwrap",
            "arg",
            "query_row",
            "filter_map",
            "as_slice",
        ];
        let mut source = String::from("#[derive(Clone, Copy)] struct Leaf; impl Leaf { fn finish(&self) {} } struct Other; impl Other { fn stop(&self) {} } struct Holder {");
        for field in fields {
            source.push_str(&format!("{field}: Leaf,"));
        }
        source.push_str(
            "} impl Holder { fn iter(&self) -> Other { Other } } fn probe(holder: Holder) {\n",
        );
        for field in fields {
            source.push_str(&format!("holder.{field}.finish(); let assigned_{field} = holder.{field}; assigned_{field}.finish();\n"));
        }
        source.push_str("holder.iter().stop(); }");
        let facts = ast::extract("field_adapter_names.rs", "rust", &source).unwrap();
        let finish = facts
            .nodes
            .iter()
            .find(|node| node.name == "finish")
            .unwrap()
            .id
            .clone();
        let stop = facts
            .nodes
            .iter()
            .find(|node| node.name == "stop")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "field_adapter_names.rs".into(),
            facts,
        )]));
        for field in fields {
            for expression in [
                format!("holder.{field}.finish"),
                format!("assigned_{field}.finish"),
            ] {
                let row = graph
                    .coverage
                    .iter()
                    .find(|row| row.expression == expression)
                    .unwrap();
                assert_eq!(row.status, "resolved", "{row:#?}");
            }
        }
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == finish)
                .count(),
            fields.len() * 2
        );
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == stop)
                .count(),
            1,
            "method application must retain its own return type"
        );
    }
    {
        let source = "struct Item; impl Item { fn finish(&self) {} } fn probe(items: Vec<Item>) { items.iter().flat_map(|item| [item]).count(); items.iter().inspect(|_| {}).count(); items.iter().fuse().count(); let changed = items.iter().flat_map(|_| [1]).collect(); for item in changed { item.finish(); } }";
        let facts = ast::extract("iterator_results.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "iterator_results.rs".into(),
            facts,
        )]));
        let counts: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.expression.ends_with(".count"))
            .collect();
        assert_eq!(counts.len(), 3);
        for row in counts {
            assert_eq!(row.status, "external", "{row:#?}");
        }
        let changed = graph
            .coverage
            .iter()
            .find(|row| row.expression == "item.finish")
            .unwrap();
        assert_eq!(
            changed.status, "unresolved",
            "callback output must remain unknown: {changed:#?}"
        );
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_rayon_manifest_authority_and_scoping() {
    {
        for (case, dependency, prelude, manifest, resolved) in [
            ("enabled", true, true, None, true),
            ("absent-dependency", false, true, None, false),
            ("absent-prelude", true, false, None, false),
            ("disabled", true, true, Some("enabled = false\n"), false),
            ("replaced", true, true, Some("enabled = true\nreplace = true\n"), false),
            ("no-return", true, true, Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['Vec', 'Slice']\nprovider = 'rayon'\nmembers = ['par_iter', 'par_iter_mut', 'into_par_iter']\n"), false),
        ] {
            let workspace = super::support::Workspace::new();
            workspace.write("Cargo.toml", if dependency { "[package]\nname = 'rayon-probe'\nversion = '0.1.0'\n[dependencies]\nrayon = '1'\n" } else { "[package]\nname = 'rayon-probe'\nversion = '0.1.0'\n" });
            if let Some(manifest) = manifest { workspace.write(".forge/frameworks/rust-cargo.toml", manifest); }
            let import = if prelude { "use rayon::prelude::*;" } else { "" };
            workspace.write("src/lib.rs", &format!("{import} struct Item; impl Item {{ fn finish(&self) {{}} }} struct Fake; impl Fake {{ fn par_iter(&self) -> Fake {{ Fake }} }} fn probe(mut items: Vec<Item>, slice: &[Item], fake: Fake) {{ let constructed = Vec::from([1]); constructed.par_iter().count(); let stream = items.par_iter(); stream.map(|item| item).count(); items.par_iter_mut().filter(|_| true).count(); items.into_par_iter().map(|_| 1).count(); slice.par_iter().map(|item| item).count(); fake.par_iter().count(); let changed = slice.par_iter().map(|_| 1).collect(); for item in changed {{ item.finish(); }} }}"));
            let db = workspace.build();
            let statuses: Vec<(String, String)> = db.prepare("SELECT x.expression,c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='src/lib.rs' AND x.expression LIKE '%.count'").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            let entry: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) WHERE x.expression='items.par_iter'", [], |row| row.get(0)).unwrap();
            assert_eq!(entry, if resolved || case == "no-return" { "external" } else { "unresolved" }, "{case}: extension entry authority");
            assert_eq!(statuses.len(), 6, "{case}: {statuses:?}");
            for (expression, status) in statuses {
                let expected = if resolved && !expression.starts_with("fake.") { "external" } else { "unresolved" };
                assert_eq!(status, expected, "{case}: {expression}");
            }
            let payload: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) WHERE x.expression='item.finish'", [], |row| row.get(0)).unwrap();
            assert_eq!(payload, "unresolved", "{case}: unproven parallel callback result");
        }
    }

    {
        for (case, import, expected) in [
            ("outer-only", "", "unresolved"),
            ("explicit-child", "use rayon::prelude::*;", "external"),
            ("use-super", "use super::*;", "external"),
        ] {
            for layout in ["inline", "file", "nested", "nested-bridge", "nested-lib"] {
                let separate_file = layout == "file";
                let workspace = super::support::Workspace::new();
                workspace.write("Cargo.toml", "[package]\nname = 'rayon-scope-probe'\nversion = '0.1.0'\n[dependencies]\nrayon = '1'\n");
                let child = format!("{import} pub fn probe(items: Vec<i32>, slice: &[i32]) {{ items.par_iter().count(); slice.par_iter().count(); }}");
                if separate_file {
                    workspace.write("src/lib.rs", "use rayon::prelude::*; mod child;");
                    workspace.write("src/child.rs", &child);
                } else {
                    let child = if layout.starts_with("nested") {
                        let bridge = if layout == "nested-bridge" {
                            "use super::*;"
                        } else {
                            ""
                        };
                        let name = if layout == "nested-lib" {
                            "lib"
                        } else {
                            "child"
                        };
                        format!("mod middle {{ {bridge} mod {name} {{ {child} }} }}")
                    } else {
                        format!("mod child {{ {child} }}")
                    };
                    workspace.write("src/lib.rs", &format!("use rayon::prelude::*; {child}"));
                }
                let db = workspace.build();
                let statuses: Vec<(String, String)> = db.prepare("SELECT x.expression,c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) WHERE x.expression LIKE '%.par_iter' OR x.expression LIKE '%.count'").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(
                    statuses.len(),
                    4,
                    "{case}, file={separate_file}: {statuses:?}"
                );
                for (expression, status) in statuses {
                    let expected =
                        if matches!(layout, "nested" | "nested-lib") && case == "use-super" {
                            "unresolved"
                        } else {
                            expected
                        };
                    assert_eq!(status, expected, "{case}, layout={layout}: {expression}");
                }
            }
        }
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_ecosystem_packs_resolution() {
    // Task 6 contract_revision: 6 Seam Proof (Ecosystem Crates & Wildcard Resilience)
    {
        let eco_source = r#"
use git2::Repository;
use walkdir::WalkDir;
use chrono::Utc;

fn test_ecosystem_packs() {
    let repo = Repository::open(".").unwrap();
    let _head = repo.head();
    let head = repo.head().unwrap();
    let commit = head.peel_to_commit().unwrap();
    let tree = commit.tree().unwrap();
    let index = repo.index().unwrap();
    tree.len();
    index.len();
    let now = Utc::now();
    now.timestamp();
    for entry in WalkDir::new(".").into_iter().filter_map(|e| e.ok()) {
        let _p = entry.path();
        entry.file_name();
        entry.file_type();
        entry.depth();
    }
}
"#;
        let eco_facts =
            ast::extract("ecosystem_test.rs", "rust", eco_source).expect("eco extraction failed");
        let eco_graph = linker::link(&std::collections::BTreeMap::from([(
            "ecosystem_test.rs".to_owned(),
            eco_facts,
        )]));
        let head_cov = eco_graph
            .coverage
            .iter()
            .find(|c| c.expression == "repo.head")
            .expect("missing coverage for repo.head");
        assert_eq!(
            head_cov.status, "external",
            "repo.head must resolve to external via git2 ecosystem pack: {head_cov:#?}"
        );
        let entry_cov = eco_graph
            .coverage
            .iter()
            .find(|c| c.expression == "entry.path")
            .expect("missing coverage for entry.path");
        assert_eq!(
            entry_cov.status, "external",
            "entry.path must resolve to external via walkdir DirEntry: {entry_cov:#?}"
        );
        for expression in [
            "head.peel_to_commit",
            "commit.tree",
            "tree.len",
            "index.len",
            "now.timestamp",
            "entry.file_name",
            "entry.file_type",
            "entry.depth",
        ] {
            let coverage = eco_graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(coverage.status, "external", "{coverage:#?}");
        }
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_workspace_compat_shims_and_gap_probe() {
    {
        let workspace = super::support::Workspace::new();
        workspace.write(
            "tests/common/mod.rs",
            "#[path = \"workspace.rs\"] mod workspace; pub use workspace::Workspace;",
        );
        workspace.write(
            "tests/common/workspace.rs",
            include_str!("../common/workspace.rs"),
        );
        workspace.write("tests/acdd/mod.rs", "pub(crate) mod support;");
        workspace.write("tests/acdd/support.rs", include_str!("../acdd/support.rs"));
        workspace.write("Cargo.toml", "[package]\nname = 'rev7-gap-probe'\nversion = '0.1.0'\n[dependencies]\nserde_json = '1'\nserde_yaml = '0.9'\nrusqlite = '0.32'\ntempfile = '3'\n");
        workspace.write(
            "tests/rev7_gap_probe.rs",
            r#"
mod common;
mod acdd;
mod rev7_client;
use rev7_client::StdioClientRev7;
use acdd::support::ScopedWorkspace;
use common::Workspace;

fn probe_rev7_gaps() {
    let workspace = acdd::support::ScopedWorkspace::new("probe");
    workspace.write("a.rs", "");
    workspace.0.join("child");
    workspace.1.write("wrapped.rs", "");

    let b = Workspace::new();
    let r = Workspace::new();
    for w in [&b, &r] {
        w.write("loop.rs", "");
    }
    let pair = (&b, &r);
    let (first, second) = pair;
    first.write("first.rs", "");
    second.write("second.rs", "");

    let mut cmd = std::process::Command::new("git");
    let output = cmd.current_dir(".").output().unwrap();
    output.status.success();
    output.stdout.len();
    let json = serde_json::from_str::<serde_json::Value>("{}").unwrap();
    json.as_object();
    let yaml = serde_yaml::from_str::<serde_yaml::Value>("{}").unwrap();
    yaml.as_mapping();
}
fn unknown_array(factory: impl Fn() -> Workspace) {
    let known = Workspace::new();
    for unknown in [&known, &factory()] {
        unknown.write("unknown.rs", "");
    }
}
fn database(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let mut statement = conn.prepare("SELECT 1")?;
    statement.query([])?;
    let mut unwrapped = conn.prepare("SELECT 1").unwrap();
    unwrapped.query([]).unwrap();
    Ok(())
}
fn transaction(tx: &rusqlite::Transaction<'_>) {
    tx.query_row("SELECT 1", [], |transaction_row| transaction_row.get::<_, i64>(0));
}
struct Outer {}
impl Outer { fn outer(&self) {} }
struct Inner {}
impl Inner { fn inner(&self) {} }
fn lexical(flag: bool) {
    let value = Outer {};
    if flag {
        let value = Inner {};
        value.inner();
        let (tuple_local, _) = (Outer {}, ());
        tuple_local.outer();
    }
    value.outer();
    for _ in [1] {
        let nested = Outer {};
        nested.outer();
    }
    let owned = "known".to_owned();
    "known".len();
    owned.len();
    std::path::Path::new(".").join("child").exists();
    String::from_utf8_lossy(&[]).len();
    u32::try_from(1u64);
}
struct GenericHolder<'a> { leaf: LeafRev7, label: &'a str }
struct LeafRev7 {}
impl LeafRev7 { fn finish(&self) {} }
impl<'a> GenericHolder<'a> { fn run(&self) { self.leaf.finish(); } }
type ClientRev7 = StdioClientRev7;
type ChainedClientRev7 = ClientRev7;
type CycleARev7 = CycleBRev7;
type CycleBRev7 = CycleARev7;
fn next_gaps(response: serde_json::Value, metadata: std::fs::Metadata, unrelated: UnknownRev7) {
    let client = ChainedClientRev7::new();
    client.payload();
    response["result"]["tools"].as_array();
    let indexed = response["result"]["tools"];
    indexed.as_array();
    unrelated["result"].as_array();
    CycleARev7::new();
    metadata.file_type().is_file();
    std::time::Instant::now().elapsed().as_millis();
    tempfile::Builder::new().prefix("probe").tempdir().unwrap().path();
}
struct BatchFacts<'a> { nodes: Vec<LeafRev7>, leaf: &'a LeafRev7 }
impl<'a> BatchFacts<'a> {
    fn shared(&self) {
        let this = self;
        this.leaf.finish();
        self.nodes.iter().next().unwrap().finish();
    }
    fn mutable(&mut self) {
        let that = self;
        that.nodes.iter_mut().next().unwrap().finish();
    }
}
fn batch_parameters<'a>(plain: &LeafRev7, mutable: &mut LeafRev7, named: &'a LeafRev7,
    anonymous: &'_ LeafRev7, facts: &BatchFacts<'a>, path: &'a std::path::Path,
    nodes: &[LeafRev7], optional: &Option<Vec<LeafRev7>>, result: &Result<LeafRev7, ()>) {
    plain.finish(); mutable.finish(); named.finish(); anonymous.finish();
    facts.leaf.finish();
    facts.nodes.iter().next().unwrap().finish();
    nodes.into_iter().next().unwrap().finish();
    optional.as_deref().unwrap().iter().next().unwrap().finish();
    result.as_ref().unwrap().finish();
    path.join("child").exists();
    let collection: Vec<LeafRev7> = Vec::new();
    collection.as_ref().iter().next().unwrap().finish();
}
fn batch_unknown<T>(unknown: &T) { unknown.finish(); }
struct BatchMaker {}
impl BatchMaker {
    fn current() -> Self { Self {} }
    fn ready(&self) {}
    fn new() -> LeafRev7 { LeafRev7 {} }
}
fn batch_tuple() -> (LeafRev7, Outer) { (LeafRev7 {}, Outer {}) }
struct MissingConstructor {}
impl MissingConstructor { fn finish(&self) {} }
struct __rust_closure {}
fn batch_six() {
    let maker = BatchMaker::current(); maker.ready();
    let changed = BatchMaker::new(); changed.finish();
    let (tuple_leaf, mut tuple_outer) = batch_tuple();
    tuple_leaf.finish(); tuple_outer.outer();
    let git = |args: &[&str]| {};
    git(&["status"]);
    let invoke = || LeafRev7 {};
    let again = invoke;
    invoke(); again();
    let unproven_result = invoke(); unproven_result.finish();
    unproven_result();
    let missing = MissingConstructor::new(); missing.finish();
    let fake = __rust_closure {}; fake();
    let number = 1; number();
}
struct BatchHub {}
impl BatchHub { fn leaf(&self) -> LeafRev7 { LeafRev7 {} } }
fn make_hub() -> BatchHub { BatchHub {} }
fn result_hub() -> Result<BatchHub, ()> { Ok(BatchHub {}) }
struct Collision { finish: LeafRev7 }
impl Collision { fn finish(&self) {} fn inspect(&self) { let _ = &self.finish; self.finish(); } }
fn batch_seven<'a>(facts: &BatchFacts<'a>, collision: &Collision, name: &str, target: &std::path::PathBuf) -> Result<(), ()> {
    let _ = &facts.nodes;
    let _ = &facts.leaf;
    let _ = &collision.finish;
    collision.finish();
    facts.leaf();
    make_hub().leaf().finish();
    result_hub()?.leaf().finish();
    name.trim().is_empty();
    target.parent().unwrap().exists();
    std::env::temp_dir().join("child").exists();
    Ok(())
}
"#,
        );
        workspace.write("tests/rev7_client.rs", "pub struct StdioClientRev7 {} impl StdioClientRev7 { pub fn new() -> Self { Self {} } pub fn payload(&self) {} }");
        workspace.write(
            "tests/acdd/mod.rs",
            "pub(crate) mod support; mod tuple_probe;",
        );
        workspace.write("tests/acdd/tuple_probe.rs", "use super::support::*; fn probe() { let (root, _, _) = fixture(); root.write(\"tuple.rs\", \"\"); root.0.join(\"child\"); }");
        let db = workspace.build();
        let write_status: String = db.query_row(
            "SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/rev7_gap_probe.rs' AND x.expression='workspace.write'",
            [],
            |row| row.get(0),
        ).unwrap();
        let debug: Vec<(String,String,String)> = db.prepare("SELECT x.expression,c.status,e.evidence FROM resolution_coverage c JOIN coverage_evidence e USING(evidence_id) JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/rev7_gap_probe.rs'").unwrap().query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(write_status, "resolved", "acdd::support::ScopedWorkspace::new must propagate receiver to workspace.write: {debug:#?}");
        for expression in [
            "workspace.0.join",
            "workspace.1.write",
            "w.write",
            "first.write",
            "second.write",
            "cmd.current_dir",
            "output.status.success",
            "output.stdout.len",
            "json.as_object",
            "yaml.as_mapping",
            "value.inner",
            "value.outer",
            "tuple_local.outer",
            "nested.outer",
            "transaction_row.get",
            "owned.len",
            "\"known\".len",
            "std::path::Path::new(\".\").join(\"child\").exists",
            "String::from_utf8_lossy(&[]).len",
            "u32::try_from",
            "self.leaf.finish",
            "ChainedClientRev7::new",
            "client.payload",
            "response[\"result\"][\"tools\"].as_array",
            "indexed.as_array",
            "metadata.file_type().is_file",
            "std::time::Instant::now().elapsed().as_millis",
            "tempfile::Builder::new().prefix(\"probe\").tempdir().unwrap().path",
            "this.leaf.finish",
            "self.nodes.iter().next().unwrap().finish",
            "that.nodes.iter_mut().next().unwrap().finish",
            "plain.finish",
            "mutable.finish",
            "named.finish",
            "anonymous.finish",
            "facts.leaf.finish",
            "facts.nodes.iter().next().unwrap().finish",
            "nodes.into_iter().next().unwrap().finish",
            "optional.as_deref().unwrap().iter().next().unwrap().finish",
            "result.as_ref().unwrap().finish",
            "path.join(\"child\").exists",
            "collection.as_ref().iter().next().unwrap().finish",
            "maker.ready",
            "changed.finish",
            "tuple_leaf.finish",
            "tuple_outer.outer",
            "git",
            "invoke",
            "again",
            "self.nodes",
            "self.leaf",
            "collision.finish",
            "make_hub().leaf().finish",
            "result_hub()?.leaf().finish",
            "name.trim().is_empty",
            "target.parent().unwrap().exists",
            "std::env::temp_dir().join(\"child\").exists",
        ] {
            let (_, status, _) = debug
                .iter()
                .find(|(expr, _, _)| expr == expression)
                .unwrap_or_else(|| panic!("missing {expression}: {debug:#?}"));
            assert!(
                matches!(status.as_str(), "resolved" | "external"),
                "{expression}: {debug:#?}"
            );
        }
        for expression in ["unrelated[\"result\"].as_array", "CycleARev7::new", "unknown.finish", "unproven_result.finish", "unproven_result", "missing.finish", "fake", "number"] {
            let (_, status, _) = debug.iter().find(|(expr, _, _)| expr == expression).unwrap();
            assert_eq!(status, "unresolved", "{expression}: {debug:#?}");
        }
        let field_reads: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='inspect' AND e.kind='references' AND d.kind='field' AND d.name='finish'", [], |row| row.get(0)).unwrap();
        assert_eq!(field_reads, 1, "field reads must prefer the declared field over a same-name method");
        let field_calls: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='batch_seven' AND e.kind='calls' AND d.kind='field'", [], |row| row.get(0)).unwrap();
        assert_eq!(field_calls, 0, "a field read must not manufacture a callable edge");
        for expression in ["statement.query", "unwrapped.query"] {
            assert_eq!(
                debug
                    .iter()
                    .find(|(expr, _, _)| expr == expression)
                    .unwrap()
                    .1,
                "external",
                "{debug:#?}"
            );
        }
        assert_eq!(
            debug
                .iter()
                .find(|(expr, _, _)| expr == "unknown.write")
                .unwrap()
                .1,
            "unresolved",
            "unknown array elements must not be discarded: {debug:#?}"
        );
        let tuple_status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/acdd/tuple_probe.rs' AND x.expression='root.write'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            tuple_status, "resolved",
            "imported tuple-returning fixture must retain each proven element"
        );
    }
}

#[cfg(feature = "lang-go")]
#[test]
fn test_go_ast_extractor() {
    let source = r#"
package main

import "fmt"

type Server struct {
    port int
}

func (s *Server) Start() {
    fmt.Println(s.port)
}

func Run() {
    s := &Server{port: 8080}
    s.Start()
}
"#;
    let facts = ast::extract("main.go", "go", source).expect("go extraction failed");
    assert!(facts.nodes.iter().any(|n| n.name == "Server"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "Start"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "Run"));
}

#[cfg(feature = "lang-python")]
#[test]
fn python_member_access_callee_does_not_emit_duplicate_reference() {
    let source = r#"
class ServiceClient:
    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}
"#;
    let facts = ast::extract("service.py", "python", source).expect("python extraction failed");
    let send_request_refs: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.expression == "self._send_request")
        .collect();
    assert_eq!(
        send_request_refs.len(),
        1,
        "Expected exactly 1 relation for self._send_request, got {:?}",
        send_request_refs
    );
    assert_eq!(send_request_refs[0].kind, "calls");

    let workspace = super::support::Workspace::new();
    let scope = format!("project_{}", "s".repeat(211));
    workspace.write(
        format!("{scope}/pyproject.toml"),
        "[project]\nname = 'scope-probe'\nversion = '0.1.0'\n",
    );
    let dependencies =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(&workspace),
        );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut scope_copies = Vec::new();
    for count in [1, 32] {
        let path = format!("{scope}/service.py");
        let source = format!(
            "class ServiceClient:\n    def fetch(self, endpoint: str) -> dict:\n{}    def _send_request(self, endpoint: str) -> dict:\n        return {{'status': 'ok'}}\n",
            "        self._send_request(endpoint)\n".repeat(count),
        );
        let facts = ast::extract(&path, "python", &source).unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .filter(|reference| reference.expression == "self._send_request")
                .count(),
            count,
        );
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "_send_request")
            .unwrap()
            .id
            .clone();
        let all = std::collections::BTreeMap::from([(path, facts)]);
        let (graph, copies) = pool.install(|| {
            let _ = contextunity_forge_mcp::engine::linker::link_with_registry(
                &all,
                None,
                Some(&workspace),
                &dependencies,
            );
            scope_allocations::measure(scope.len(), || {
                contextunity_forge_mcp::engine::linker::link_with_registry(
                    &all,
                    None,
                    Some(&workspace),
                    &dependencies,
                )
            })
        });
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target)
                .count(),
            count,
            "every member call must retain its exact target",
        );
        assert_eq!(
            graph
                .coverage
                .iter()
                .filter(|coverage| coverage.expression == "self._send_request"
                    && coverage.status == "resolved")
                .count(),
            count,
        );
        scope_copies.push(copies);
    }
    assert!(
        scope_copies[1] <= scope_copies[0],
        "assertion failed: package-scope preparation must be per file, not per member reference: {scope_copies:?}",
    );
}
