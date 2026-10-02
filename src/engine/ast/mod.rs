pub(crate) mod relations;
pub(crate) mod routes;
use crate::core::models::*;
use crate::core::typed_facts::{FlowStore, TypedFacts};
use crate::engine::languages::{self, FileContext, LanguageProfile, SyntaxContext};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node as Syntax, Parser, Tree};
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
impl Extraction<'_> {
    fn visit(
        &mut self,
        node: Syntax<'_>,
        scopes: &mut Vec<String>,
        owner: &str,
        facts: &mut Facts,
    ) {
        let (source, path, language, module, offset) = (
            self.source,
            self.path,
            self.profile.id(),
            self.module,
            self.offset,
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
                            &self.file.shadowed_require_scopes,
                        ));
                }
                if !self.file.lazy_exports.is_empty() {
                    module.details["lazy_exports"] = json!(self.file.lazy_exports);
                }
                if !self.file.exports.is_empty() {
                    module.details["exports"] = json!(self.file.exports);
                }
                let mut value_flow = self.profile.value_flow(node, source);
                value_flow.offset_lines(offset);
                if !value_flow.is_empty() {
                    if let Some(flows) = &mut self.flows {
                        if let Some(previous) = flows.take(&module.id) {
                            value_flow.bindings.extend(previous.bindings);
                            value_flow.fields.extend(previous.fields);
                        }
                    } else if let Ok(previous) = crate::core::semantic::ValueFlowFacts::deserialize(
                        &module.details["value_flow"],
                    ) {
                        value_flow.bindings.extend(previous.bindings);
                        value_flow.fields.extend(previous.fields);
                    }
                    value_flow.bindings.sort_by_key(|binding| binding.position);
                    value_flow.fields.sort_by_key(|field| field.position);
                    if let Some(flows) = &mut self.flows {
                        module.details["value_flow"] = serde_json::Value::Null;
                        flows.insert(module.id.clone(), value_flow);
                    } else {
                        module.details["value_flow"] = json!(value_flow);
                    }
                }
                let scope_bindings = self.profile.bindings(node, source);
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
        let mut child_owner = owner.to_owned();
        let mut pushed = false;
        if let Some(kind) = self.profile.symbol_with_source(node, source) {
            let name = self
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
            let prefix = self.profile.node_prefix(kind);
            let line = node.start_position().row + offset + 1;
            let mut id = format!("{prefix}:{path}:{line}:{name}");
            if !self.ids.insert(id.clone()) {
                id.push_str(&format!(":{}", node.start_position().column));
                self.ids.insert(id.clone());
            }
            self.symbols.insert(node.id(), id.clone());
            let metadata = self.profile.metadata(node, source, &name, &self.file);
            let bindings = self.profile.bindings(node, source);
            let mut details = json!({"receiver_name":metadata.receiver_name,"bindings":bindings.all,"rebindings":bindings.rebindings,"default_export":metadata.default_export,"doc":self.profile.doc_comment(node,source),"decorators":metadata.decorators,"bases":metadata.bases,"receiver":metadata.receiver,"signature":declaration_signature(node,source),"async":metadata.is_async});
            details["column"] = json!(node.start_position().column);
            if let Some(receiver_type) = metadata.receiver_type {
                details["receiver_type"] = json!(receiver_type);
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
            let mut value_flow = self.profile.value_flow(node, source);
            if value_flow.return_type.is_none() {
                value_flow.return_type = metadata.return_type;
            }
            value_flow.offset_lines(offset);
            if !value_flow.is_empty() {
                if let Some(flows) = &mut self.flows {
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
            facts.nodes.push(Node {
                id: id.clone(),
                kind: kind.into(),
                name: name.clone(),
                qualname: qualified(module, scopes, &name),
                path: path.into(),
                line,
                end_line: node.end_position().row + offset + 1,
                is_test: is_test(path) || self.profile.test_attribute(node, source),
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
            let shadowed_require_scopes = std::sync::Arc::clone(&self.file.shadowed_require_scopes);
            self.profile.extract_relations(
                &SyntaxContext {
                    node,
                    source,
                    owner: &id,
                    offset,
                    shadowed_require_scopes: &shadowed_require_scopes,
                },
                facts,
            );
            child_owner = id;
            scopes.push(name);
            pushed = true;
        }
        let shadowed_require_scopes = std::sync::Arc::clone(&self.file.shadowed_require_scopes);
        let ctx = SyntaxContext {
            node,
            source,
            owner: &child_owner,
            offset,
            shadowed_require_scopes: &shadowed_require_scopes,
        };
        self.profile.extract_imports(&ctx, facts);
        self.profile.extract_calls(&ctx, facts);
        self.profile.extract_mutations(&ctx, facts);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, scopes, &child_owner, facts);
        }
        self.profile.extract_routes(&ctx, facts, &self.symbols);
        if pushed {
            scopes.pop();
        }
    }
}
/// Performs extract.
pub fn extract(path: &str, language: &str, source: &str) -> Result<Facts> {
    Ok(extract_typed(path, language, source)?.into_public())
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

    let mut typed = TypedFacts {
        facts,
        flows: FlowStore::default(),
    };
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

/// The search match horizon value.
pub const SEARCH_MATCH_HORIZON: usize = 10_000;

struct SearchBudget {
    deadline: Option<std::time::Instant>,
    remaining: usize,
    exhausted: bool,
}

impl SearchBudget {
    fn unbounded() -> Self {
        Self {
            deadline: None,
            remaining: 0,
            exhausted: false,
        }
    }

    fn bounded(query_deadline: std::time::Instant) -> Self {
        let file_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        Self {
            deadline: Some(query_deadline.min(file_deadline)),
            remaining: 1_000_000,
            exhausted: false,
        }
    }

    fn step(&mut self) -> bool {
        let Some(deadline) = self.deadline else {
            return true;
        };
        if self.exhausted || self.remaining == 0 || std::time::Instant::now() >= deadline {
            self.exhausted = true;
            return false;
        }
        self.remaining -= 1;
        true
    }

    fn prepare_parser(&mut self, parser: &mut Parser) -> bool {
        if !self.step() {
            return false;
        }
        if let Some(deadline) = self.deadline {
            parser.set_timeout_micros(
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_micros()
                    .max(1) as u64,
            );
        }
        true
    }
}

fn structural_match(
    pattern: Syntax<'_>,
    target: Syntax<'_>,
    ps: &str,
    source: &str,
    captures: &mut serde_json::Map<String, serde_json::Value>,
    budget: &mut SearchBudget,
) -> bool {
    if !budget.step() {
        return false;
    }
    let token = text(pattern, ps);
    if token.starts_with("__FORGE_META_") && pattern.named_child_count() == 0 {
        let name = token.trim_start_matches("__FORGE_META_");
        let value = json!(text(target, source));
        return match captures.get(name) {
            Some(old) => *old == value,
            None => {
                captures.insert(name.into(), value);
                true
            }
        };
    }
    if pattern.kind() != target.kind() {
        return false;
    }
    if pattern.child_count() == 0 {
        return token == text(target, source);
    }
    let mut pc = pattern.walk();
    let p: Vec<_> = pattern
        .children(&mut pc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    let mut tc = target.walk();
    let t: Vec<_> = target
        .children(&mut tc)
        .filter(|n| !n.kind().contains("comment"))
        .collect();
    fn sequence(
        p: &[Syntax<'_>],
        t: &[Syntax<'_>],
        ps: &str,
        ts: &str,
        caps: &mut serde_json::Map<String, serde_json::Value>,
        budget: &mut SearchBudget,
    ) -> bool {
        if !budget.step() {
            return false;
        }
        if p.is_empty() {
            return t.is_empty();
        }
        let token = text(p[0], ps);
        if token.starts_with("__FORGE_MANY_") {
            for count in 0..=t.len() {
                if !budget.step() {
                    return false;
                }
                let mut branch = caps.clone();
                let name = token.trim_start_matches("__FORGE_MANY_");
                let value = if count == 0 {
                    ""
                } else {
                    &ts[t[0].start_byte()..t[count - 1].end_byte()]
                };
                if branch.get(name).is_some_and(|v| v != value) {
                    continue;
                }
                branch.insert(name.into(), json!(value));
                if sequence(&p[1..], &t[count..], ps, ts, &mut branch, budget) {
                    *caps = branch;
                    return true;
                }
            }
            return false;
        }
        if t.is_empty() || !structural_match(p[0], t[0], ps, ts, caps, budget) {
            return false;
        }
        sequence(&p[1..], &t[1..], ps, ts, caps, budget)
    }
    sequence(&p, &t, ps, source, captures, budget)
}
/// Performs search.
pub fn search(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    Ok(search_range(
        source,
        path,
        language,
        pattern,
        0,
        limit,
        SearchBudget::unbounded(),
    )?
    .items)
}

/// Represents search page data.
pub struct SearchPage {
    /// The items value.
    pub items: Vec<serde_json::Value>,
    /// The matched value.
    pub matched: usize,
    /// Whether complete applies.
    pub complete: bool,
    /// Whether work limited applies.
    pub work_limited: bool,
}

impl SearchPage {
    fn limited(items: Vec<serde_json::Value>, matched: usize) -> Self {
        Self {
            items,
            matched,
            complete: false,
            work_limited: true,
        }
    }
}

/// Performs search page.
pub fn search_page(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    offset: usize,
    limit: usize,
    deadline: std::time::Instant,
) -> Result<SearchPage> {
    anyhow::ensure!(
        limit > 0
            && offset
                .checked_add(limit)
                .is_some_and(|end| end <= SEARCH_MATCH_HORIZON),
        "AST search offset + limit must be 1..=10000; narrow path or pattern"
    );
    search_range(
        source,
        path,
        language,
        pattern,
        offset,
        limit,
        SearchBudget::bounded(deadline),
    )
}

fn normalize_search_pattern(pattern: &str) -> String {
    let mut normalized = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut count = 1;
            while chars.peek() == Some(&'$') {
                chars.next();
                count += 1;
            }
            normalized.push_str(if count > 1 {
                "__FORGE_MANY_"
            } else {
                "__FORGE_META_"
            });
        } else {
            normalized.push(c);
        }
    }
    normalized
}

pub(crate) fn pattern_symbol_name(pattern: &str, language: &str) -> Result<Option<String>> {
    let profile = languages::require(language)?;
    let mut normalized = normalize_search_pattern(pattern);
    profile.prepare_pattern(&mut normalized);
    let mut parser = profile.create_parser("__forge_pattern__")?;
    let tree = parser.parse(&normalized, None).context("invalid pattern")?;
    anyhow::ensure!(
        !tree.root_node().has_error(),
        "pattern is not valid {language} syntax"
    );
    let mut node = tree.root_node();
    while node.named_child_count() == 1 && profile.pattern_wrapper(node.kind()) {
        node = node.named_child(0).context("empty pattern")?;
    }
    if profile.symbol_with_source(node, &normalized).is_none() {
        return Ok(None);
    }
    Ok(profile
        .symbol_name(node, &normalized)
        .filter(|name| !name.contains("__FORGE_"))
        .map(str::to_owned))
}

fn search_range(
    source: &str,
    path: &str,
    language: &str,
    pattern: &str,
    offset: usize,
    limit: usize,
    mut budget: SearchBudget,
) -> Result<SearchPage> {
    let bounded = budget.deadline.is_some();
    let mut normalized = normalize_search_pattern(pattern);
    let profile = languages::require(language)?;
    let partial_body = profile.prepare_pattern(&mut normalized);
    let mut parser = profile.create_parser(path)?;
    if !budget.prepare_parser(&mut parser) {
        return Ok(SearchPage::limited(Vec::new(), 0));
    }
    let pt: Tree = match parser.parse(&normalized, None) {
        Some(tree) => tree,
        None if bounded => return Ok(SearchPage::limited(Vec::new(), 0)),
        None => bail!("invalid pattern"),
    };
    if pt.root_node().has_error() {
        bail!("pattern is not valid {language} syntax");
    }
    let mut pn = pt.root_node();
    while pn.named_child_count() == 1 && profile.pattern_wrapper(pn.kind()) {
        pn = pn.named_child(0).context("empty pattern")?;
    }
    if !budget.prepare_parser(&mut parser) {
        return Ok(SearchPage::limited(Vec::new(), 0));
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None if bounded => return Ok(SearchPage::limited(Vec::new(), 0)),
        None => bail!("parse cancelled"),
    };
    let mut stack = vec![tree.root_node()];
    let mut matches = Vec::new();
    let mut matched_count = 0;
    let mut visited = 0;
    while let Some(n) = stack.pop() {
        visited += 1;
        if !budget.step() || (bounded && visited > 100_000) {
            return Ok(SearchPage::limited(matches, matched_count));
        }
        let mut captures = serde_json::Map::new();
        let matched = if partial_body && n.kind() == pn.kind() {
            let mut pc = pn.walk();
            let mut nc = n.walk();
            let a: Vec<_> = pn
                .children(&mut pc)
                .filter(|x| x.kind() != "block")
                .collect();
            let b: Vec<_> = n
                .children(&mut nc)
                .filter(|x| x.kind() != "block")
                .collect();
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    structural_match(*a, b, &normalized, source, &mut captures, &mut budget)
                })
        } else {
            structural_match(pn, n, &normalized, source, &mut captures, &mut budget)
        };
        if budget.exhausted {
            return Ok(SearchPage::limited(matches, matched_count));
        }
        if matched {
            matched_count += 1;
            if matched_count > offset {
                if matches.len() == limit {
                    return Ok(SearchPage {
                        items: matches,
                        matched: matched_count,
                        complete: false,
                        work_limited: false,
                    });
                }
                matches.push(json!({"path":path,"line":n.start_position().row+1,"end_line":n.end_position().row+1,"text":text(n,source),"captures":captures}));
                if !bounded && matches.len() >= limit {
                    return Ok(SearchPage {
                        items: matches,
                        matched: matched_count,
                        complete: false,
                        work_limited: false,
                    });
                }
            }
        }
        let mut c = n.walk();
        let children: Vec<_> = n.named_children(&mut c).collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(SearchPage {
        items: matches,
        matched: matched_count,
        complete: true,
        work_limited: false,
    })
}
