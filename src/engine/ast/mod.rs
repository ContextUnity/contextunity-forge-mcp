pub(crate) mod relations;
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
pub struct ScopeBindings {
    /// The all value.
    pub all: Vec<String>,
    /// The rebindings value.
    pub rebindings: Vec<String>,
}
pub(crate) fn scope_bindings(node: Syntax<'_>, source: &str) -> ScopeBindings {
    let is_callable = matches!(
        node.kind(),
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "method_definition"
            | "method_declaration"
            | "arrow_function"
            | "function_expression"
            | "lambda"
            | "closure_expression"
            | "func_literal"
    );
    let is_module = matches!(
        node.kind(),
        "module" | "program" | "source_file" | "class_definition"
    );
    if !is_callable && !is_module {
        return ScopeBindings {
            all: Vec::new(),
            rebindings: Vec::new(),
        };
    }
    fn names<'a>(node: Syntax<'_>, source: &'a str, out: &mut HashSet<&'a str>) {
        match node.kind() {
            #[cfg(feature = "lang-rust")]
            "tuple_struct_pattern" | "struct_pattern" | "ref_pattern" | "captured_pattern" => {
                languages::rust::patterns::bindings(node, |binding| {
                    out.insert(text(binding, source));
                });
            }
            "identifier" | "shorthand_property_identifier_pattern" => {
                out.insert(text(node, source));
            }
            "default_parameter"
            | "typed_default_parameter"
            | "required_parameter"
            | "optional_parameter"
            | "parameter"
            | "parameter_declaration"
            | "variadic_parameter_declaration" => {
                for field in ["name", "pattern"] {
                    let mut c = node.walk();
                    for child in node.children_by_field_name(field, &mut c) {
                        names(child, source, out);
                    }
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(n) = node.child_by_field_name("left") {
                    names(n, source, out);
                }
            }
            "pair_pattern" => {
                if let Some(n) = node.child_by_field_name("value") {
                    names(n, source, out);
                }
            }
            "typed_parameter" => {
                if let Some(n) = node.named_child(0) {
                    if Some(n) != node.child_by_field_name("type") {
                        names(n, source, out);
                    }
                }
            }
            "parameters"
            | "lambda_parameters"
            | "as_pattern_target"
            | "formal_parameters"
            | "parameter_list"
            | "tuple_pattern"
            | "list_pattern"
            | "pattern_list"
            | "expression_list"
            | "array_pattern"
            | "object_pattern"
            | "rest_pattern"
            | "list_splat_pattern"
            | "dictionary_splat_pattern"
            | "slice_pattern"
            | "reference_pattern"
            | "mut_pattern" => {
                let mut c = node.walk();
                for child in node.named_children(&mut c) {
                    names(child, source, out);
                }
            }
            _ => {}
        }
    }
    let mut param_names = HashSet::new();
    if is_callable {
        for field in ["parameters", "parameter"] {
            if let Some(parameters) = node.child_by_field_name(field) {
                names(parameters, source, &mut param_names);
            }
        }
    }
    let mut body_names = HashSet::new();
    let body = node
        .child_by_field_name("body")
        .or_else(|| is_module.then_some(node));
    if let Some(body_node) = body {
        let mut stack = vec![body_node];
        while let Some(n) = stack.pop() {
            if n != body_node
                && matches!(
                    n.kind(),
                    "function_definition"
                        | "lambda"
                        | "class_definition"
                        | "function_declaration"
                        | "function_item"
                        | "arrow_function"
                        | "function_expression"
                        | "method_definition"
                        | "method_declaration"
                        | "closure_expression"
                        | "func_literal"
                )
            {
                continue;
            }
            let lhs = match n.kind() {
                "assignment"
                | "augmented_assignment"
                | "assignment_expression"
                | "augmented_assignment_expression"
                | "for_statement"
                | "for_in_statement"
                | "for_in_clause"
                | "short_var_declaration" => n.child_by_field_name("left"),
                "as_pattern" => n.child_by_field_name("alias"),
                "variable_declarator" | "let_declaration" | "named_expression" => n
                    .child_by_field_name("name")
                    .or_else(|| n.child_by_field_name("pattern")),
                _ => None,
            };
            let callable = n
                .child_by_field_name("value")
                .or_else(|| n.child_by_field_name("right"))
                .is_some_and(|n| {
                    matches!(
                        n.kind(),
                        "arrow_function" | "function_expression" | "lambda"
                    )
                });
            if !callable {
                if let Some(lhs) = lhs {
                    names(lhs, source, &mut body_names);
                }
            }
            if n.kind() == "delete_statement" {
                let mut c = n.walk();
                for child in n.named_children(&mut c) {
                    names(child, source, &mut body_names);
                }
            }
            let mut c = n.walk();
            stack.extend(n.named_children(&mut c));
        }
    }
    let mut all_set: std::collections::BTreeSet<String> =
        param_names.into_iter().map(str::to_owned).collect();
    let rebindings_set: std::collections::BTreeSet<String> =
        body_names.into_iter().map(str::to_owned).collect();
    all_set.extend(rebindings_set.iter().cloned());
    ScopeBindings {
        all: all_set.into_iter().collect(),
        rebindings: rebindings_set.into_iter().collect(),
    }
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

struct Extraction<'a> {
    source: &'a str,
    path: &'a str,
    profile: &'a dyn LanguageProfile,
    file: FileContext,
    module: &'a str,
    offset: usize,
    symbols: HashMap<usize, String>,
    ids: HashSet<String>,
    flows: Option<&'a mut FlowStore>,
}
struct VisitContext<'a> {
    source: &'a str,
    path: &'a str,
    profile: &'a dyn LanguageProfile,
    file: &'a FileContext,
    module: &'a str,
    offset: usize,
}
struct VisitState<'a, 'flow> {
    root_bindings: Option<ScopeBindings>,
    symbols: &'a mut HashMap<usize, String>,
    ids: &'a mut HashSet<String>,
    flows: &'a mut Option<&'flow mut FlowStore>,
}
impl Extraction<'_> {
    fn visit(
        &mut self,
        node: Syntax<'_>,
        scopes: &mut Vec<String>,
        owner: &str,
        facts: &mut Facts,
    ) {
        let root_bindings = self
            .file
            .root_bindings
            .take()
            .filter(|(root_id, _)| *root_id == node.id())
            .map(|(_, bindings)| bindings);
        let context = VisitContext {
            source: self.source,
            path: self.path,
            profile: self.profile,
            file: &self.file,
            module: self.module,
            offset: self.offset,
        };
        let mut state = VisitState {
            root_bindings,
            symbols: &mut self.symbols,
            ids: &mut self.ids,
            flows: &mut self.flows,
        };
        Self::visit_node(&context, &mut state, node, scopes, owner, facts);
    }

    fn visit_node(
        context: &VisitContext<'_>,
        state: &mut VisitState<'_, '_>,
        node: Syntax<'_>,
        scopes: &mut Vec<String>,
        owner: &str,
        facts: &mut Facts,
    ) {
        let (source, path, language, module, offset) = (
            context.source,
            context.path,
            context.profile.id(),
            context.module,
            context.offset,
        );
        if node.parent().is_none() {
            if let Some(module) = facts.nodes.iter_mut().find(|n| n.id == owner) {
                #[cfg(feature = "lang-python")]
                if language == "python" {
                    let factories =
                        crate::engine::languages::python::logging_factories(node, source, offset);
                    if factories
                        .as_array()
                        .is_some_and(|entries| !entries.is_empty())
                    {
                        module.details["python_logging_factories"] = factories;
                    }
                }
                #[cfg(feature = "lang-typescript")]
                if matches!(language, "javascript" | "typescript" | "vue") {
                    module.details["commonjs_bindings"] =
                        json!(crate::engine::languages::typescript::commonjs_bindings(
                            node,
                            source,
                            offset,
                            &context.file.shadowed_require_scopes,
                        ));
                }
                if !context.file.lazy_exports.is_empty() {
                    module.details["lazy_exports"] = json!(context.file.lazy_exports);
                }
                if !context.file.exports.is_empty() {
                    module.details["exports"] = json!(context.file.exports);
                }
                let mut value_flow =
                    context
                        .profile
                        .value_flow_with_context(node, source, context.file);
                value_flow.offset_lines(offset);
                if !value_flow.is_empty() {
                    if let Some(flows) = state.flows.as_mut() {
                        if let Some(previous) = flows.take(&module.id) {
                            value_flow.bindings.extend(previous.bindings);
                            value_flow
                                .immutable_initializers
                                .extend(previous.immutable_initializers);
                            value_flow.fields.extend(previous.fields);
                        }
                    } else if let Ok(previous) = crate::core::semantic::ValueFlowFacts::deserialize(
                        &module.details["value_flow"],
                    ) {
                        value_flow.bindings.extend(previous.bindings);
                        value_flow
                            .immutable_initializers
                            .extend(previous.immutable_initializers);
                        value_flow.fields.extend(previous.fields);
                    }
                    value_flow.bindings.sort_by_key(|binding| binding.position);
                    value_flow.immutable_initializers.sort_unstable();
                    value_flow.fields.sort_by_key(|field| field.position);
                    if let Some(flows) = state.flows.as_mut() {
                        module.details["value_flow"] = serde_json::Value::Null;
                        flows.insert(module.id.clone(), value_flow);
                    } else {
                        module.details["value_flow"] = json!(value_flow);
                    }
                }
                let scope_bindings = state
                    .root_bindings
                    .take()
                    .unwrap_or_else(|| context.profile.bindings(node, source));
                let bindings = module.details["bindings"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let bindings: std::collections::BTreeSet<String> = bindings
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .chain(scope_bindings.all)
                    .collect();
                module.details["bindings"] = json!(bindings);
                let rebindings: std::collections::BTreeSet<String> = module.details["rebindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .chain(scope_bindings.rebindings)
                    .collect();
                module.details["rebindings"] = json!(rebindings);
            }
        }
        if node.is_error() || node.is_missing() {
            let is_ts_grammar_gap = if language == "typescript" {
                let line_text = source.lines().nth(node.start_position().row).unwrap_or("");
                line_text.contains("import(")
                    || line_text.contains("readonly [")
                    || line_text.contains("readonly (")
            } else {
                false
            };
            if !is_ts_grammar_gap {
                facts.errors.push(Diagnostic {
                    path: path.into(),
                    line: node.start_position().row + offset + 1,
                    message: format!(
                        "syntax {} at column {}",
                        node.kind(),
                        node.start_position().column + 1
                    ),
                });
            }
        }
        let mut child_owner = std::borrow::Cow::Borrowed(owner);
        let mut pushed = false;
        if let Some(kind) = context.profile.symbol_with_source(node, source) {
            let name = context
                .profile
                .symbol_name(node, source)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "anonymous@{}:{}",
                        node.start_position().row + offset + 1,
                        node.start_position().column + 1
                    )
                });
            let prefix = context.profile.node_prefix(kind);
            let line = node.start_position().row + offset + 1;
            let mut id = format!("{prefix}:{path}:{line}:{name}");
            if !state.ids.insert(id.clone()) {
                id.push_str(&format!(":{}", node.start_position().column));
                state.ids.insert(id.clone());
            }
            state.symbols.insert(node.id(), id.clone());
            let (metadata, mut value_flow) =
                context
                    .profile
                    .declaration(node, source, &name, context.file);
            let prototype_owner = (language == "javascript"
                && matches!(node.kind(), "function_expression" | "arrow_function")
                && metadata.is_method == Some(true))
            .then(|| metadata.receiver_type.clone())
            .flatten();
            let qualname = if let Some(receiver) = prototype_owner.as_deref() {
                qualified(module, scopes, &format!("{receiver}.{name}"))
            } else {
                qualified(module, scopes, &name)
            };
            let bindings = context.profile.bindings(node, source);
            let mut details = json!({"receiver_name":metadata.receiver_name,"bindings":bindings.all,"rebindings":bindings.rebindings,"default_export":metadata.default_export,"doc":context.profile.doc_comment(node,source),"decorators":metadata.decorators,"bases":metadata.bases,"receiver":metadata.receiver,"signature":declaration_signature(node,source),"async":metadata.is_async});
            details["column"] = json!(node.start_position().column);
            if let Some(receiver_type) = metadata.receiver_type {
                details["receiver_type"] = json!(receiver_type);
            }
            if prototype_owner.is_some() {
                details["prototype"] = json!(true);
            }
            if language == "javascript"
                && crate::engine::languages::typescript::commonjs_default_callable(node, source)
            {
                details["commonjs_default_callable"] = json!(true);
            }
            if let Some(is_method) = metadata.is_method {
                details["is_method"] = json!(is_method);
            }
            if let Some(is_static) = metadata.is_static {
                details["is_static"] = json!(is_static);
            }
            if !metadata.param_types.is_empty() {
                details["param_types"] = json!(metadata.param_types);
            }
            if value_flow.return_type.is_none() {
                value_flow.return_type = metadata.return_type;
            }
            value_flow.offset_lines(offset);
            if !value_flow.is_empty() {
                if let Some(flows) = state.flows.as_mut() {
                    details["value_flow"] = serde_json::Value::Null;
                    flows.insert(id.clone(), value_flow);
                } else {
                    details["value_flow"] = json!(value_flow);
                }
            }
            if metadata.is_overload {
                details["is_overload"] = json!(true);
            }
            if metadata.is_stub {
                details["is_stub"] = json!(true);
            }
            if metadata.is_conditional {
                details["is_conditional"] = json!(true);
            }
            facts.nodes.push(Node {
                id: id.clone(),
                kind: kind.into(),
                name: name.clone(),
                qualname,
                path: path.into(),
                line,
                end_line: node.end_position().row + offset + 1,
                is_test: is_test(path) || context.profile.test_attribute(node, source),
                language: language.into(),
                generated: false,
                details,
            });
            facts.edges.push(Edge {
                src: owner.into(),
                dst: id.clone(),
                kind: "contains".into(),
                path: path.into(),
                line,
                evidence: node.kind().into(),
                confidence: "exact".into(),
            });
            context.profile.extract_relations(
                &SyntaxContext {
                    node,
                    source,
                    owner: &id,
                    offset,
                    shadowed_require_scopes: &context.file.shadowed_require_scopes,
                    type_checking_aliases: Some(&context.file.type_checking_aliases),
                },
                facts,
            );
            child_owner = std::borrow::Cow::Owned(id);
            scopes.push(name);
            pushed = true;
        }
        let ctx = SyntaxContext {
            node,
            source,
            owner: &child_owner,
            offset,
            shadowed_require_scopes: &context.file.shadowed_require_scopes,
            type_checking_aliases: Some(&context.file.type_checking_aliases),
        };
        context.profile.extract_imports(&ctx, facts);
        context.profile.extract_calls(&ctx, facts);
        context.profile.extract_mutations(&ctx, facts);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            Self::visit_node(context, state, child, scopes, &child_owner, facts);
        }
        context.profile.extract_routes(&ctx, facts, state.symbols);
        if pushed {
            scopes.pop();
        }
    }
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

pub(crate) fn extract_typed(path: &str, language: &str, source: &str) -> Result<TypedFacts> {
    let profile = languages::require(language)?;
    let module = profile.module_name_for_source(path, source);
    let mut facts = Facts::default();
    facts.nodes.push(Node {
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
        details: json!({}),
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
        return Ok(typed);
    }
    match language {
        "python" | "rust" | "typescript" | "javascript" => {
            languages::parse_file_typed(profile, path, source, &module, &mut typed)?;
            profile.finish(&mut typed.facts);
        }
        #[cfg(feature = "lang-html")]
        "html" => languages::html::extract_typed(path, source, &module, &mut typed)?,
        #[cfg(feature = "lang-vue")]
        "vue" => languages::vue::extract_typed(path, source, &module, &mut typed)?,
        _ => {
            profile.extract_file(path, source, &module, &mut typed.facts)?;
            profile.finish(&mut typed.facts);
            for node in &mut typed.facts.nodes {
                typed.flows.ingest(node);
            }
        }
    }
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
    extract_tree_owned(
        profile,
        root,
        path,
        source,
        module,
        &format!("module:{path}"),
        facts,
    );
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

pub(crate) fn extract_tree_owned(
    profile: &dyn LanguageProfile,
    root: Syntax<'_>,
    path: &str,
    source: &str,
    module: &str,
    owner: &str,
    facts: &mut Facts,
) {
    extract_tree_with_flows(profile, root, path, source, module, owner, facts, None);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_tree_with_flows(
    profile: &dyn LanguageProfile,
    root: Syntax<'_>,
    path: &str,
    source: &str,
    module: &str,
    owner: &str,
    facts: &mut Facts,
    flows: Option<&mut FlowStore>,
) {
    Extraction {
        source,
        path,
        profile,
        file: profile.prepare(root, source),
        module,
        offset: 0,
        symbols: HashMap::new(),
        ids: facts.nodes.iter().map(|n| n.id.clone()).collect(),
        flows,
    }
    .visit(root, &mut Vec::new(), owner, facts);
}
