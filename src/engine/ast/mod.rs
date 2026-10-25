pub(crate) mod visitor;
pub use visitor::ScopeAnalysis;
pub(crate) mod relations;
pub(crate) mod route_includes;
pub(crate) mod routes;
mod search;
use crate::core::models::*;
use crate::core::typed_facts::{FlowStore, TypedFacts};
use crate::engine::languages::{self, FileContext, LanguageProfile, SyntaxContext};
use anyhow::Result;
pub(crate) use search::pattern_symbol_name;
pub use search::{
    search, search_page, PatternErrorSpan, PatternSearchDiagnostic, SearchPage,
    SEARCH_MATCH_HORIZON,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node as Syntax, Parser};

#[cfg(feature = "lang-typescript")]
fn commonjs_default_callable(node: tree_sitter::Node<'_>, source: &str) -> bool {
    crate::engine::languages::typescript::commonjs_default_callable(node, source)
}

#[cfg(not(feature = "lang-typescript"))]
fn commonjs_default_callable(_node: tree_sitter::Node<'_>, _source: &str) -> bool {
    false
}
/// Performs parser.
pub fn parser(language: &str, path: &str) -> Result<Parser> {
    languages::require(language)?.create_parser(path)
}
/// Performs text.
pub fn text<'a>(node: Syntax<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
pub(crate) fn field<'a>(node: Syntax<'_>, source: &'a str, name: &str) -> Option<&'a str> {
    node.child_by_field_name(name).map(|n| text(n, source))
}
fn qualified(module: &str, scopes: &[String], name: &str) -> String {
    std::iter::once(module)
        .chain(scopes.iter().map(String::as_str))
        .chain(std::iter::once(name))
        .collect::<Vec<_>>()
        .join(".")
}
pub(crate) fn doc_comment(node: Syntax<'_>, source: &str) -> String {
    let mut anchor = node;
    while let Some(parent) = anchor.parent() {
        if matches!(
            parent.kind(),
            "export_statement"
                | "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
                | "decorated_definition"
        ) {
            anchor = parent;
        } else {
            break;
        }
    }
    let mut comments = Vec::new();
    let mut previous = anchor.prev_named_sibling();
    while let Some(n) = previous {
        if !n.kind().contains("comment") {
            break;
        }
        comments.push(text(n, source));
        previous = n.prev_named_sibling();
    }
    comments.reverse();
    comments.join("\n")
}
/// Represents scope bindings data.
#[derive(Debug)]
pub struct ScopeBindings {
    /// The all value.
    pub all: Vec<String>,
    /// The rebindings value.
    pub rebindings: Vec<String>,
}
pub(crate) fn scope_bindings(node: Syntax<'_>, source: &str) -> ScopeBindings {
    visitor::bindings(node, source)
}

fn declaration_signature(node: Syntax<'_>, source: &str) -> String {
    let mut c = node.walk();
    let body_node = node.child_by_field_name("body").or_else(|| {
        node.named_children(&mut c)
            .find(|ch| matches!(ch.kind(), "message_body" | "enum_body"))
    });
    let end = body_node.map_or(node.end_byte(), |b| b.start_byte());
    source[node.start_byte()..end]
        .trim()
        .trim_end_matches(';')
        .trim()
        .chars()
        .take(512)
        .collect()
}
fn declaration_signature_is_informative(node: Syntax<'_>) -> bool {
    let parameters = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("formal_parameters"));
    let has_parameters = parameters.is_none_or(|parameters| parameters.named_child_count() > 0);
    if has_parameters
        || ["return_type", "type", "type_parameters", "type_arguments"]
            .into_iter()
            .any(|field| node.child_by_field_name(field).is_some())
    {
        return true;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return true;
    };
    if !matches!(
        body.kind(),
        "block" | "statement_block" | "function_body" | "compound_statement"
    ) {
        return true;
    }
    let mut cursor = body.walk();
    let has_body = body.named_children(&mut cursor).any(|child| {
        !child.kind().contains("comment")
            && !matches!(child.kind(), "pass_statement" | "empty_statement")
    });
    has_body
}
fn declaration_signature_if_informative(node: Syntax<'_>, source: &str) -> Option<String> {
    declaration_signature_is_informative(node).then(|| declaration_signature(node, source))
}
pub(crate) fn bounded_expression(value: &str) -> String {
    if value.len() <= 512 {
        return value.into();
    }
    format!(
        "{}… [sha256:{}]",
        value.chars().take(256).collect::<String>(),
        crate::core::commitments::hash(value.as_bytes())
    )
}
pub(crate) fn symbol_name<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    let inferred = node.parent().and_then(|p| {
        if matches!(p.kind(), "variable_declarator" | "pair" | "assignment") {
            field(p, source, "name")
                .or_else(|| field(p, source, "left"))
                .or_else(|| field(p, source, "key"))
        } else {
            None
        }
    });
    field(node, source, "name")
        .or_else(|| field(node, source, "type"))
        .or(inferred)
}

/// Performs extract.
pub fn extract(path: &str, language: &str, source: &str) -> Result<Facts> {
    Ok(extract_typed(path, language, source)?.into_public())
}

fn is_trivial_source(source: &str, language: &str) -> bool {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return true;
    }
    match language {
        "python" => trimmed.lines().all(|l| {
            let s = l.trim();
            s.is_empty() || (s.starts_with('#') && !s.starts_with("# type:"))
        }),
        "rust" => trimmed.lines().all(|l| {
            let s = l.trim();
            s.is_empty() || (s.starts_with("//") && !s.starts_with("//!") && !s.starts_with("///"))
        }),
        "javascript" | "typescript" => trimmed.lines().all(|l| {
            let s = l.trim();
            s.is_empty() || (s.starts_with("//") && !s.starts_with("///"))
        }),
        _ => false,
    }
}

fn compact_node_details(facts: &mut Facts, path: &str) {
    let mut declarations_per_line = HashMap::new();
    for node in facts
        .nodes
        .iter()
        .filter(|node| node.path == path && node.kind != "module")
    {
        *declarations_per_line.entry(node.line).or_insert(0usize) += 1;
    }
    for node in facts.nodes.iter_mut().filter(|node| node.path == path) {
        let empty = if let Some(details) = node.details.as_object_mut() {
            details.retain(|key, value| match key.as_str() {
                "value_flow" => !value.is_null(),
                "bindings" | "rebindings" => {
                    !value.as_array().is_some_and(|values| values.is_empty())
                }
                "async" | "default_export" | "is_method" | "is_public" => {
                    value != &serde_json::Value::Bool(false)
                }
                "is_static" => value != &serde_json::Value::Bool(false),
                _ => true,
            });
            let has_other_details = details.keys().any(|key| key != "column");
            let keep_column = !matches!(node.kind.as_str(), "function" | "method")
                || has_other_details
                || details
                    .get("column")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|column| column > 0)
                || declarations_per_line
                    .get(&node.line)
                    .is_some_and(|count| *count > 1);
            if !keep_column {
                details.remove("column");
            }
            details.is_empty()
        } else {
            false
        };
        if empty && node.native_semantic.is_none() {
            node.details = serde_json::Value::Null;
        }
    }
}

pub(crate) fn extract_typed(path: &str, language: &str, source: &str) -> Result<TypedFacts> {
    let registry = languages::dependency_registry::DependencyRegistry::bundled_framework_rules();
    extract_typed_with_registry(path, language, source, &registry)
}

pub(crate) fn extract_typed_with_registry(
    path: &str,
    language: &str,
    source: &str,
    registry: &languages::dependency_registry::DependencyRegistry,
) -> Result<TypedFacts> {
    let language_context = registry.language_context_for_path(path);
    let profile = languages::require(language)?;
    let module = profile.module_name_for_source(path, source);
    let mut facts = Facts::default();
    facts.nodes.push(Node {
        native_semantic: None,
        id: format!("module:{path}"),
        kind: "module".into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        qualname: module.clone(),
        path: path.into(),
        line: 1,
        end_line: source.lines().count().max(1),
        is_test: is_test(path),
        language: language.into(),
        generated: false,
        details: serde_json::Value::Null,
    });

    let bytes = source.len();
    if bytes > 1024 {
        facts.nodes.reserve((bytes / 512).clamp(4, 256));
        facts.edges.reserve((bytes / 256).clamp(8, 512));
        facts.references.reserve((bytes / 256).clamp(8, 512));
    }

    let mut typed = TypedFacts {
        facts,
        flows: FlowStore::default(),
    };
    if is_trivial_source(source, language) {
        profile.finish(&mut typed.facts);
        languages::semantic::file_providers(&mut typed.facts, &language_context);
        typed.facts.semantic.mark_complete();
        compact_node_details(&mut typed.facts, path);
        return Ok(typed);
    }
    match language {
        "python" | "rust" | "typescript" | "javascript" => {
            languages::parse_file_typed(
                profile,
                path,
                source,
                &module,
                &mut typed,
                &language_context,
            )?;
            profile.finish(&mut typed.facts);
        }
        #[cfg(feature = "lang-html")]
        "html" => languages::html::extract_typed_with_context(
            path,
            source,
            &module,
            &mut typed,
            &language_context,
        )?,
        #[cfg(feature = "lang-vue")]
        "vue" => {
            languages::vue::extract_typed(path, source, &module, &mut typed, &language_context)?
        }
        _ => {
            profile.extract_file(path, source, &module, &mut typed.facts)?;
            profile.finish(&mut typed.facts);
            for (index, node) in typed.facts.nodes.iter_mut().enumerate() {
                typed.flows.ingest(node);
                typed.facts.semantic.ingest(index, node);
            }
        }
    }
    languages::semantic::file_providers(&mut typed.facts, &language_context);
    // Only generated declarations retain a parser-owned JSON flow projection.
    // Body scopes publish directly into their canonical native store.
    for index in std::mem::take(&mut typed.facts.pending_flow_nodes) {
        let node = typed
            .facts
            .nodes
            .get_mut(index)
            .expect("generated flow target follows node remapping");
        if typed.flows.state(node).is_none() {
            typed.flows.ingest(node);
        }
    }
    for node in &mut typed.facts.nodes {
        crate::core::typed_facts::NodeSemanticEntry::ingest(node);
    }
    typed.facts.semantic.mark_complete();
    compact_node_details(&mut typed.facts, path);
    Ok(typed)
}

pub(crate) fn extract_tree(
    profile: &dyn LanguageProfile,
    root: Syntax<'_>,
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
) {
    let registry = languages::dependency_registry::DependencyRegistry::bundled_framework_rules();
    let language_context = registry.language_context_for_path(path);
    extract_tree_with_language_context(
        profile,
        root,
        path,
        source,
        module,
        &format!("module:{path}"),
        facts,
        None,
        &language_context,
    );
    facts.semantic.mark_complete();
    compact_node_details(facts, path);
}

pub(crate) fn parse_island(
    parser: &mut tree_sitter::Parser,
    input: &str,
    offset: usize,
    position: tree_sitter::Point,
) -> Option<tree_sitter::Tree> {
    let mut padded = String::with_capacity(input.len() + 1);
    padded.push('\n');
    padded.push_str(input);
    let mut tree = parser.parse(&padded, None)?;
    // A virtual prefix keeps parent traversal in the same coordinate system.
    tree.edit(&tree_sitter::InputEdit {
        start_byte: 0,
        old_end_byte: 1,
        new_end_byte: offset,
        start_position: tree_sitter::Point::new(0, 0),
        old_end_position: tree_sitter::Point::new(1, 0),
        new_end_position: position,
    });
    Some(tree)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_tree_with_language_context(
    profile: &dyn LanguageProfile,
    root: Syntax<'_>,
    path: &str,
    source: &str,
    module: &str,
    owner: &str,
    facts: &mut Facts,
    flows: Option<&mut FlowStore>,
    language_context: &std::sync::Arc<languages::LanguageContext>,
) -> ScopeBindings {
    let rule_ir = language_context.bind_rule_ir(&root.language());
    let route_context = if profile.route_extraction_enabled() {
        routes::RouteContext::prepare(root, source, language_context)
    } else {
        routes::RouteContext::default()
    };
    let bindings = visitor::SinglePassExtractor {
        source,
        path,
        profile,
        language_context,
        rule_ir,
        route_context,
        file: profile.prepare_with_context(root, source, language_context),
        module,
        offset: 0,
        symbols: HashMap::new(),
        ids: facts.nodes.iter().map(|n| n.id.clone()).collect(),
        flows,
    }
    .visit(root, &mut Vec::new(), owner, facts);
    bindings
}
