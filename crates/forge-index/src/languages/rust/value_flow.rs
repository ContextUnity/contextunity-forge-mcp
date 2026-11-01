use crate::core::semantic::{
    BindingFact, FieldFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts,
};
use crate::{
    core::models::Facts,
    engine::ast::{relations, text},
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};
use tree_sitter::Node;

pub(super) fn type_expr(node: Node<'_>, source: &str) -> TypeExpr {
    let generic_names = generic_names(node, source);
    type_expr_inner(node, source, &generic_names, 0)
}

fn type_expr_inner(
    node: Node<'_>,
    source: &str,
    generic_names: &HashSet<&str>,
    depth: usize,
) -> TypeExpr {
    if depth >= crate::engine::languages::MAX_VALUE_FLOW_TYPE_DEPTH {
        return TypeExpr::Unknown;
    }
    if node.kind() == "reference_type" {
        let Some(inner) = node.child_by_field_name("type") else {
            return TypeExpr::Unknown;
        };
        let mut cursor = node.walk();
        let mutable = node
            .named_children(&mut cursor)
            .any(|child| child.kind() == "mutable_specifier")
            || text(node, source)
                .split_whitespace()
                .any(|part| part == "mut");
        return TypeExpr::Applied {
            base: if mutable { "&mut" } else { "&" }.to_owned(),
            args: vec![type_expr_inner(inner, source, generic_names, depth + 1)],
        };
    }
    if matches!(node.kind(), "array_type" | "slice_type") {
        let Some(element) = node
            .child_by_field_name("element")
            .or_else(|| node.named_child(0))
        else {
            return TypeExpr::Unknown;
        };
        let fixed_array =
            node.kind() == "array_type" && node.child_by_field_name("length").is_some();
        return TypeExpr::Applied {
            base: if fixed_array {
                "__rust_array_type"
            } else {
                "[]"
            }
            .to_owned(),
            args: vec![type_expr_inner(element, source, generic_names, depth + 1)],
        };
    }
    if let Some(name) = trait_object_name(node, source) {
        return TypeExpr::Named { name };
    }
    if matches!(
        node.kind(),
        "type_identifier" | "scoped_type_identifier" | "primitive_type"
    ) {
        let name = text(node, source).trim();
        if name != "_" && !generic_root(node, source, generic_names) {
            return TypeExpr::Named {
                name: name.to_owned(),
            };
        }
    }
    if node.kind() == "generic_type" {
        let Some(base) = node
            .child_by_field_name("type")
            .and_then(|node| static_name(node, source))
        else {
            return TypeExpr::Unknown;
        };
        if generic_root(node, source, generic_names) {
            return TypeExpr::Unknown;
        }
        let Some(arguments) = node.child_by_field_name("type_arguments") else {
            return TypeExpr::Unknown;
        };
        let mut cursor = arguments.walk();
        let mut args = Vec::new();
        let mut has_lifetime = false;
        for argument in arguments.named_children(&mut cursor) {
            if argument.kind() == "lifetime" {
                has_lifetime = true;
                continue;
            }
            if args.len() == 4 {
                return TypeExpr::Unknown;
            }
            args.push(type_expr_inner(argument, source, generic_names, depth + 1));
        }
        if !args.is_empty() {
            return TypeExpr::Applied { base, args };
        }
        if has_lifetime {
            return TypeExpr::Named { name: base };
        }
    }
    TypeExpr::Unknown
}

fn trait_object_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut syntax = text(node, source).trim();
    while syntax.starts_with('(') && syntax.ends_with(')') {
        syntax = syntax[1..syntax.len().checked_sub(1)?].trim();
    }
    let bounds = syntax.strip_prefix("dyn ")?;
    let principal = bounds.split('+').next()?.trim();
    if principal.is_empty()
        || !principal
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':'))
    {
        return None;
    }
    Some(principal.to_owned())
}

fn generic_root(mut node: Node<'_>, source: &str, names: &HashSet<&str>) -> bool {
    if names.is_empty() {
        return false;
    }
    for _ in 0..8 {
        node = match node.kind() {
            "generic_type" => match node.child_by_field_name("type") {
                Some(base) => base,
                None => return true,
            },
            "scoped_type_identifier" | "scoped_identifier" => {
                match node.child_by_field_name("path") {
                    Some(path) => path,
                    None => return false,
                }
            }
            "type_identifier" | "identifier" | "primitive_type" => {
                return names.contains(text(node, source));
            }
            _ => return false,
        };
    }
    true
}

fn static_name(node: Node<'_>, source: &str) -> Option<String> {
    matches!(
        node.kind(),
        "identifier" | "scoped_identifier" | "type_identifier" | "scoped_type_identifier"
    )
    .then(|| text(node, source).trim().to_owned())
}

pub(super) fn callee_name<'a>(mut node: Node<'_>, source: &'a str) -> Option<Cow<'a, str>> {
    const MAX_SEGMENTS: usize = 8;
    const MAX_BYTES: usize = 512;

    struct Parts<'a> {
        names: [Option<(&'static str, &'a str)>; MAX_SEGMENTS],
        len: usize,
        bytes: usize,
    }

    impl<'a> Parts<'a> {
        fn push(&mut self, separator: &'static str, name: &'a str) -> Option<()> {
            if self.len == MAX_SEGMENTS || name.is_empty() {
                return None;
            }
            self.bytes = self
                .bytes
                .checked_add(separator.len())?
                .checked_add(name.len())?;
            if self.bytes > MAX_BYTES
                || !name
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
            {
                return None;
            }
            self.names[self.len] = Some((separator, name));
            self.len += 1;
            Some(())
        }

        fn collect(&mut self, node: Node<'_>, source: &'a str, depth: usize) -> Option<()> {
            if depth >= MAX_SEGMENTS || node.has_error() || node.is_missing() {
                return None;
            }
            match node.kind() {
                "identifier" | "type_identifier" | "self" | "super" | "crate" => {
                    self.push("", text(node, source))
                }
                "scoped_identifier" => {
                    if let Some(path) = node.child_by_field_name("path") {
                        self.collect(path, source, depth + 1)?;
                    }
                    let name = node.child_by_field_name("name")?;
                    if !matches!(name.kind(), "identifier" | "super") {
                        return None;
                    }
                    self.push("::", text(name, source))
                }
                "field_expression" => {
                    self.collect(node.child_by_field_name("value")?, source, depth + 1)?;
                    let field = node.child_by_field_name("field")?;
                    if field.kind() != "field_identifier" {
                        return None;
                    }
                    self.push(".", text(field, source))
                }
                _ => None,
            }
        }
    }

    if node.has_error() || node.is_missing() {
        return None;
    }
    if node.kind() == "generic_function" {
        node = node.child_by_field_name("function")?;
    }
    let mut parts = Parts {
        names: [None; MAX_SEGMENTS],
        len: 0,
        bytes: 0,
    };
    parts.collect(node, source, 0)?;
    let original = text(node, source);
    if original.len() == parts.bytes {
        return Some(Cow::Borrowed(original));
    }
    let mut canonical = String::with_capacity(parts.bytes);
    for (separator, name) in parts.names.into_iter().take(parts.len).flatten() {
        canonical.push_str(separator);
        canonical.push_str(name);
    }
    Some(Cow::Owned(canonical))
}

const MAX_ADAPTER_CHAIN_DEPTH: usize = 7;

pub(super) fn value(node: Node<'_>, source: &str) -> ValueExpr {
    let file_facts = file_macro_facts(node, source);
    let shadows = collect_ancestor_shadows(node, source, &file_facts);
    value_depth(node, source, 0, &shadows)
}

fn value_depth(
    node: Node<'_>,
    source: &str,
    depth: usize,
    shadowed_macros: &HashSet<&'static str>,
) -> ValueExpr {
    if depth >= MAX_ADAPTER_CHAIN_DEPTH && node.kind() == "call_expression" {
        return ValueExpr::Unknown;
    }
    if depth > MAX_ADAPTER_CHAIN_DEPTH {
        return ValueExpr::Unknown;
    }
    match node.kind() {
        "try_expression" => {
            node.named_child(0)
                .map_or(ValueExpr::Unknown, |inner| ValueExpr::Project {
                    value: Box::new(value_depth(inner, source, depth + 1, shadowed_macros)),
                    member: "?".to_owned(),
                })
        }
        "parenthesized_expression" => node
            .named_child(0)
            .map_or(ValueExpr::Unknown, |inner| value_depth(inner, source, depth, shadowed_macros)),
        "struct_expression" => node
            .child_by_field_name("name")
            .and_then(|name| static_name(name, source))
            .map_or(ValueExpr::Unknown, |callee| ValueExpr::Construct { callee }),
        "macro_invocation" => {
            let callee = node
                .child_by_field_name("macro")
                .map(|m| text(m, source).trim());
            match callee {
                Some("vec") => {
                    if shadowed_macros.contains("vec") {
                        ValueExpr::Unknown
                    } else {
                        ValueExpr::Construct {
                            callee: "Vec".to_string(),
                        }
                    }
                }
                Some("format") => {
                    if shadowed_macros.contains("format") {
                        ValueExpr::Unknown
                    } else {
                        ValueExpr::Construct {
                            callee: "String".to_string(),
                        }
                    }
                }
                _ => ValueExpr::Unknown,
            }
        }
        "call_expression" => {
            let Some(mut function) = node.child_by_field_name("function") else {
                return ValueExpr::Unknown;
            };
            if function.kind() == "generic_function" {
                let Some(inner) = function.child_by_field_name("function") else {
                    return ValueExpr::Unknown;
                };
                function = inner;
            }
            if function.kind() == "field_expression" {
                if let (Some(receiver), Some(member)) = (
                    function.child_by_field_name("value"),
                    function.child_by_field_name("field"),
                ) {
                    let member_name = text(member, source);
                    if matches!(
                        member_name,
                        "unwrap"
                            | "expect"
                            | "ok"
                            | "as_ref"
                            | "as_mut"
                            | "as_deref"
                            | "cloned"
                            | "copied"
                            | "iter"
                            | "into_iter"
                            | "next"
                            | "map"
                            | "enumerate"
                            | "filter"
                            | "take"
                            | "skip"
                            | "step_by"
                            | "collect"
                    ) {
                        let inner = value_depth(receiver, source, depth + 1, shadowed_macros);
                        if matches!(inner, ValueExpr::Unknown) {
                            return ValueExpr::Unknown;
                        }
                        return ValueExpr::Project {
                            value: Box::new(inner),
                            member: member_name.to_owned(),
                        };
                    }
                }
            }
            callee_name(function, source).map_or(ValueExpr::Unknown, |callee| ValueExpr::Call {
                callee: callee.into_owned(),
                site: None,
            })
        }
        "identifier" | "scoped_identifier" => ValueExpr::Alias {
            name: text(node, source).to_owned(),
        },
        _ => ValueExpr::Unknown,
    }
}

pub(crate) struct Collector<'source, 'tree> {
    root: Node<'tree>,
    body: Option<Node<'tree>>,
    source: &'source str,
    file_facts: std::sync::Arc<FileMacroFacts>,
    facts: ValueFlowFacts,
    generic_names: Option<HashSet<&'source str>>,
    states: Vec<(bool, bool)>,
    block_shadows: Vec<HashSet<&'static str>>,
}

impl<'source, 'tree> Collector<'source, 'tree> {
    pub(crate) fn new(node: Node<'tree>, source: &'source str) -> Self {
        let file_facts = file_macro_facts(node, source);
        let root_shadows = collect_ancestor_shadows(node, source, &file_facts);
        let block_shadows = vec![root_shadows];
        let mut facts = ValueFlowFacts::default();
        let mut generics = None;
        let mut resolve = |ty: Node<'_>| {
            let names = generics.get_or_insert_with(|| generic_names(node, source));
            type_expr_inner(ty, source, names, 0)
        };
        if node.kind() == "type_item" {
            facts.alias_type = node.child_by_field_name("type").map(&mut resolve);
        }
        if matches!(node.kind(), "function_item" | "function_signature_item") {
            facts.return_type = node.child_by_field_name("return_type").map(|ty| {
                if is_async(node, source) {
                    TypeExpr::Unknown
                } else {
                    resolve(ty)
                }
            });
            if let Some(parameters) = node.child_by_field_name("parameters") {
                let mut cursor = parameters.walk();
                for parameter in parameters.named_children(&mut cursor) {
                    let (Some(pattern), Some(annotation)) = (
                        parameter.child_by_field_name("pattern"),
                        parameter.child_by_field_name("type"),
                    ) else {
                        continue;
                    };
                    let resolved = resolve(annotation);
                    let mut nominal = annotation;
                    while nominal.kind() == "reference_type" {
                        let Some(inner) = nominal.child_by_field_name("type") else {
                            break;
                        };
                        nominal = inner;
                    }
                    if nominal.kind() == "primitive_type" && text(nominal, source) != "str" {
                        continue;
                    }
                    match &resolved {
                        TypeExpr::Applied { .. } => {}
                        TypeExpr::Named { .. } => {}
                        TypeExpr::Unknown => continue,
                    }
                    let pattern = if pattern.kind() == "mut_pattern" {
                        pattern.named_child(0).unwrap_or(pattern)
                    } else {
                        pattern
                    };
                    if pattern.kind() != "identifier" {
                        continue;
                    }
                    let position = parameter.end_position();
                    facts.bindings.push(BindingFact {
                        name: text(pattern, source).to_owned(),
                        position: SourcePosition {
                            line: position.row + 1,
                            column: position.column,
                        },
                        value: ValueExpr::Annotated {
                            type_expr: resolved,
                        },
                        conditional: false,
                    });
                }
            }
        }
        let Some(body) = node.child_by_field_name("body") else {
            return Self {
                root: node,
                body: None,
                source,
                file_facts,
                facts,
                generic_names: generics,
                states: Vec::new(),
                block_shadows,
            };
        };
        if node.kind() == "struct_item" {
            let mut push_field = |name, at: Node<'_>, ty| {
                let position = at.start_position();
                facts.fields.push(FieldFact {
                    name,
                    position: SourcePosition {
                        line: position.row + 1,
                        column: position.column,
                    },
                    value: ValueExpr::Annotated {
                        type_expr: resolve(ty),
                    },
                    conditional: false,
                });
            };
            let mut cursor = body.walk();
            if body.kind() == "ordered_field_declaration_list" {
                for (ordinal, ty) in body.children_by_field_name("type", &mut cursor).enumerate() {
                    push_field(ordinal.to_string(), ty, ty);
                }
            } else {
                for field in body.named_children(&mut cursor) {
                    if field.kind() != "field_declaration" {
                        continue;
                    }
                    if let (Some(name), Some(ty)) = (
                        field.child_by_field_name("name"),
                        field.child_by_field_name("type"),
                    ) {
                        push_field(text(name, source).to_owned(), field, ty);
                    }
                }
            }
            return Self {
                root: node,
                body: None,
                source,
                file_facts,
                facts,
                generic_names: generics,
                states: Vec::new(),
                block_shadows,
            };
        }
        Self {
            root: node,
            body: Some(body),
            source,
            file_facts,
            facts,
            generic_names: generics,
            states: Vec::new(),
            block_shadows,
        }
    }

    fn current_shadows(&self) -> HashSet<&'static str> {
        let mut shadows = HashSet::new();
        for scope in &self.block_shadows {
            shadows.extend(scope);
        }
        shadows
    }

    pub(crate) fn enter(&mut self, current: Node<'tree>) {
        let source = self.source;
        let body = self.body;
        let (parent_active, inherited) = self.states.last().copied().unwrap_or_default();
        let active = body == Some(current)
            || (parent_active
                && !matches!(
                    current.kind(),
                    "function_item"
                        | "closure_expression"
                        | "impl_item"
                        | "trait_item"
                        | "mod_item"
                ));
        let conditional = inherited
            || (body != Some(current)
                && matches!(
                    current.kind(),
                    "block"
                        | "if_expression"
                        | "match_expression"
                        | "while_expression"
                        | "for_expression"
                        | "loop_expression"
                ));
        self.states.push((active, conditional));
        if !active {
            return;
        }
        if current.kind() == "block" {
            let block_set = self
                .file_facts
                .blocks
                .get(&current.id())
                .map(|b| b.imports.clone())
                .unwrap_or_default();
            self.block_shadows.push(block_set);
        }
        if current.kind() == "macro_definition" {
            if let Some(m_name) = current.child_by_field_name("name") {
                let name = text(m_name, source).trim();
                if name == "vec" || name == "format" {
                    if let Some(last) = self.block_shadows.last_mut() {
                        if name == "vec" {
                            last.insert("vec");
                        } else {
                            last.insert("format");
                        }
                    }
                }
            }
        }
        if matches!(
            current.kind(),
            "let_declaration" | "assignment_expression" | "compound_assignment_expr"
        ) {
            let pattern = current
                .child_by_field_name("pattern")
                .or_else(|| current.child_by_field_name("left"));
            if let Some(pattern) = pattern {
                let mut emit = |pattern: Node<'_>| {
                    let position = current.end_position();
                    let inferred = if current.kind() == "let_declaration"
                        && current.child_by_field_name("value").is_some()
                        && current
                            .child_by_field_name("pattern")
                            .is_some_and(|pattern| {
                                matches!(pattern.kind(), "identifier" | "mut_pattern")
                            }) {
                        current
                            .child_by_field_name("type")
                            .map(|ty| ValueExpr::Annotated {
                                type_expr: type_expr_inner(
                                    ty,
                                    source,
                                    self.generic_names
                                        .get_or_insert_with(|| generic_names(self.root, source)),
                                    0,
                                ),
                            })
                            .unwrap_or_else(|| {
                                let shadows = self.current_shadows();
                                value_depth(
                                    current.child_by_field_name("value").expect("value checked"),
                                    source,
                                    0,
                                    &shadows,
                                )
                            })
                    } else {
                        ValueExpr::Unknown
                    };
                    self.facts.bindings.push(BindingFact {
                        name: text(pattern, source).to_owned(),
                        position: SourcePosition {
                            line: position.row + 1,
                            column: position.column,
                        },
                        value: inferred,
                        conditional,
                    });
                };
                if current.kind() == "let_declaration" {
                    super::patterns::bindings(pattern, emit);
                } else {
                    let mut names = vec![pattern];
                    while let Some(pattern) = names.pop() {
                        if pattern.kind() == "identifier" {
                            emit(pattern);
                        } else {
                            let mut cursor = pattern.walk();
                            names.extend(pattern.named_children(&mut cursor));
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn exit(&mut self, node: Node<'tree>) {
        if node.kind() == "block" && self.block_shadows.len() > 1 {
            self.block_shadows.pop();
        }
        self.states.pop();
    }

    pub(crate) fn finish(mut self) -> ValueFlowFacts {
        self.facts.bindings.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.name.cmp(&right.name))
        });
        self.facts.fields.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.name.cmp(&right.name))
        });
        self.facts
    }
}

pub(super) fn extract(node: Node<'_>, source: &str) -> ValueFlowFacts {
    let mut collector = Collector::new(node, source);
    crate::engine::ast::visitor::walk(node, &mut |node, entering| {
        if entering {
            collector.enter(node);
        } else {
            collector.exit(node);
        }
    });
    collector.finish()
}

pub(super) fn is_async(node: Node<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    let is_async = node.named_children(&mut cursor).any(|child| {
        child.kind() == "function_modifiers"
            && text(child, source)
                .split_whitespace()
                .any(|modifier| modifier == "async")
    });
    is_async
}

fn generic_names<'a>(node: Node<'_>, source: &'a str) -> HashSet<&'a str> {
    let mut generic_names = HashSet::new();
    let mut ancestor = Some(node);
    while let Some(scope) = ancestor {
        if let Some(parameters) = scope.child_by_field_name("type_parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let name = match parameter.kind() {
                    "type_identifier" => Some(parameter),
                    "type_parameter" | "const_parameter" => parameter.child_by_field_name("name"),
                    "constrained_type_parameter" | "optional_type_parameter" => parameter
                        .child_by_field_name("left")
                        .or_else(|| parameter.child_by_field_name("name")),
                    _ => None,
                };
                if let Some(name) = name {
                    let name = if name.kind() == "constrained_type_parameter" {
                        name.child_by_field_name("left")
                    } else {
                        Some(name)
                    };
                    if let Some(name) = name {
                        generic_names.insert(text(name, source));
                    }
                }
            }
        }
        ancestor = scope.parent();
    }
    generic_names
}

pub(super) fn type_references(
    facts: &mut Facts,
    owner: &str,
    node: Node<'_>,
    source: &str,
    line: usize,
) {
    let generic_names = generic_names(node, source);
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "lifetime" | "lifetime_parameter" => {}
            "scoped_type_identifier" => {
                relations::reference(facts, owner, text(node, source), "references", line)
            }
            "type_identifier" | "identifier" => {
                let name = text(node, source);
                if name != "_" && !name.starts_with('\'') && !generic_names.contains(name) {
                    relations::reference(facts, owner, name, "references", line);
                }
            }
            _ => {
                let mut cursor = node.walk();
                stack.extend(node.named_children(&mut cursor));
            }
        }
    }
}

#[derive(Clone, Default, Debug)]
pub(crate) struct BlockMacroFacts {
    pub(crate) defs: Vec<(usize, &'static str)>,
    pub(crate) imports: HashSet<&'static str>,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct ModuleMacroFacts {
    pub(crate) defs: Vec<(usize, &'static str)>,
    pub(crate) imports: HashSet<&'static str>,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct FileMacroFacts {
    pub(crate) root_macro_defs: Vec<(usize, &'static str)>,
    pub(crate) root_macro_imports: HashSet<&'static str>,
    pub(crate) modules: HashMap<usize, ModuleMacroFacts>,
    pub(crate) blocks: HashMap<usize, BlockMacroFacts>,
}

fn get_file_root(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn module_of<'a>(mut node: Node<'a>) -> Node<'a> {
    if node.kind() == "declaration_list" {
        if let Some(parent) = node.parent() {
            if parent.kind() == "mod_item" {
                return node;
            }
        }
    }
    if node.kind() == "source_file" {
        return node;
    }
    while let Some(parent) = node.parent() {
        if parent.kind() == "declaration_list" {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "mod_item" {
                    return parent;
                }
            }
        }
        node = parent;
    }
    node
}

fn parent_module<'a>(mod_node: Node<'a>, file_root: Node<'a>) -> Option<Node<'a>> {
    if mod_node == file_root {
        return None;
    }
    let mod_item = mod_node.parent()?;
    let enclosing = mod_item.parent()?;
    Some(module_of(enclosing))
}

fn find_child_module<'a>(body: Node<'a>, mod_name: &str, source: &str) -> Option<Node<'a>> {
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "mod_item" {
            if let Some(name_node) = child.child_by_field_name("name") {
                if text(name_node, source).trim() == mod_name {
                    return child.child_by_field_name("body");
                }
            }
        }
    }
    None
}

fn resolve_module_path<'a, 's>(
    context: Node<'a>,
    path: &'s str,
    source: &str,
    file_root: Node<'a>,
) -> Option<(Node<'a>, &'s str)> {
    let enc_mod = module_of(context);
    let (mut current, clean_path) = if let Some(stripped) = path.strip_prefix("crate::") {
        (file_root, stripped)
    } else if let Some(stripped) = path.strip_prefix("self::") {
        (enc_mod, stripped)
    } else if path.starts_with("super::") {
        let mut curr_mod = enc_mod;
        let mut remaining = path;
        while let Some(stripped) = remaining.strip_prefix("super::") {
            curr_mod = parent_module(curr_mod, file_root)?;
            remaining = stripped;
        }
        (curr_mod, remaining)
    } else {
        let first_seg = path.split("::").next().unwrap_or(path);
        if find_child_module(enc_mod, first_seg, source).is_some() || !path.contains("::") {
            (enc_mod, path)
        } else {
            (file_root, path)
        }
    };

    let parts: Vec<&str> = clean_path.split("::").collect();
    if parts.is_empty() {
        return None;
    }
    let target_name = parts[parts.len() - 1];
    let mod_parts = &parts[..parts.len() - 1];

    for &mod_name in mod_parts {
        current = find_child_module(current, mod_name, source)?;
    }

    Some((current, target_name))
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum MacroResolution {
    KnownMacro,
    KnownNonMacro,
    UnknownOrExhausted,
}

fn module_has_macro(
    body: Node<'_>,
    target_name: &str,
    source: &str,
    file_root: Node<'_>,
    visited: &mut HashSet<(usize, String)>,
    depth: usize,
) -> MacroResolution {
    if depth > 8 || !visited.insert((body.id(), target_name.to_string())) {
        return MacroResolution::UnknownOrExhausted;
    }

    let mut saw_exhausted = false;
    let mut saw_non_macro = false;
    let mut cursor = body.walk();
    for item in body.named_children(&mut cursor) {
        if item.kind() == "macro_definition" {
            if let Some(m_name) = item.child_by_field_name("name") {
                if text(m_name, source).trim() == target_name {
                    return MacroResolution::KnownMacro;
                }
            }
        } else if matches!(
            item.kind(),
            "function_item"
                | "function_signature_item"
                | "struct_item"
                | "enum_item"
                | "type_item"
                | "const_item"
                | "static_item"
                | "trait_item"
                | "union_item"
                | "mod_item"
        ) {
            if let Some(name_node) = item.child_by_field_name("name") {
                if text(name_node, source).trim() == target_name {
                    saw_non_macro = true;
                }
            }
        } else if item.kind() == "foreign_mod_item" {
            let mut f_cursor = item.walk();
            for f_child in item.named_children(&mut f_cursor) {
                if f_child.kind() == "declaration_list" {
                    let mut d_cursor = f_child.walk();
                    for d_child in f_child.named_children(&mut d_cursor) {
                        if let Some(name_node) = d_child.child_by_field_name("name") {
                            if text(name_node, source).trim() == target_name {
                                saw_non_macro = true;
                            }
                        }
                    }
                } else if let Some(name_node) = f_child.child_by_field_name("name") {
                    if text(name_node, source).trim() == target_name {
                        saw_non_macro = true;
                    }
                }
            }
        } else if item.kind() == "extern_crate_declaration" {
            let bound_name = item
                .child_by_field_name("alias")
                .or_else(|| item.child_by_field_name("name"));
            if let Some(name_node) = bound_name {
                if text(name_node, source).trim() == target_name {
                    saw_non_macro = true;
                }
            }
        } else if item.kind() == "use_declaration" {
            if let Some(arg) = item.child_by_field_name("argument") {
                let mut mod_imports = Vec::new();
                super::collect_use(arg, source, "", &mut mod_imports);
                for (re_path, re_alias) in mod_imports {
                    if re_alias == target_name {
                        let clean = re_path
                            .strip_prefix("crate::")
                            .or_else(|| re_path.strip_prefix("self::"))
                            .unwrap_or(&re_path);
                        if clean.starts_with("std::")
                            || clean.starts_with("core::")
                            || clean.starts_with("alloc::")
                        {
                            saw_non_macro = true;
                            continue;
                        }
                        if let Some((target_body, actual_target)) =
                            resolve_module_path(body, &re_path, source, file_root)
                        {
                            match module_has_macro(
                                target_body,
                                actual_target,
                                source,
                                file_root,
                                visited,
                                depth + 1,
                            ) {
                                MacroResolution::KnownMacro => return MacroResolution::KnownMacro,
                                MacroResolution::UnknownOrExhausted => saw_exhausted = true,
                                MacroResolution::KnownNonMacro => saw_non_macro = true,
                            }
                        } else {
                            saw_exhausted = true;
                        }
                    }
                }
            }
        }
    }

    if saw_non_macro && !saw_exhausted {
        MacroResolution::KnownNonMacro
    } else {
        MacroResolution::UnknownOrExhausted
    }
}

fn is_imported_target_macro(
    context: Node<'_>,
    path: &str,
    source: &str,
    file_root: Node<'_>,
) -> bool {
    let clean = path
        .strip_prefix("crate::")
        .or_else(|| path.strip_prefix("self::"))
        .unwrap_or(path);
    if clean.starts_with("std::") || clean.starts_with("core::") || clean.starts_with("alloc::") {
        return false;
    }
    if let Some((target_body, target_name)) = resolve_module_path(context, path, source, file_root) {
        let mut visited = HashSet::new();
        let res = module_has_macro(target_body, target_name, source, file_root, &mut visited, 0);
        return matches!(
            res,
            MacroResolution::KnownMacro | MacroResolution::UnknownOrExhausted
        );
    }
    true
}

fn build_file_macro_facts(root: Node<'_>, source: &str) -> FileMacroFacts {
    let file_root = get_file_root(root);
    let mut facts = FileMacroFacts::default();

    let mut stack = vec![file_root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if node == file_root {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "macro_definition" {
                    if let Some(m_name) = child.child_by_field_name("name") {
                        let name = text(m_name, source).trim();
                        if name == "vec" {
                            facts.root_macro_defs.push((child.end_byte(), "vec"));
                        } else if name == "format" {
                            facts.root_macro_defs.push((child.end_byte(), "format"));
                        }
                    }
                } else if child.kind() == "use_declaration" {
                    if let Some(argument) = child.child_by_field_name("argument") {
                        let mut imports = Vec::new();
                        super::collect_use(argument, source, "", &mut imports);
                        for (path, alias) in imports {
                            if (alias == "vec" || alias == "format")
                                && !path.starts_with("std::")
                                && !path.starts_with("core::")
                                && !path.starts_with("alloc::")
                                && is_imported_target_macro(file_root, &path, source, file_root)
                            {
                                facts.root_macro_imports.insert(if alias == "vec" { "vec" } else { "format" });
                            }
                        }
                    }
                } else {
                    stack.push(child);
                }
            }
        } else if kind == "declaration_list" {
            let mut mod_facts = ModuleMacroFacts::default();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "macro_definition" {
                    if let Some(m_name) = child.child_by_field_name("name") {
                        let name = text(m_name, source).trim();
                        if name == "vec" {
                            mod_facts.defs.push((child.end_byte(), "vec"));
                        } else if name == "format" {
                            mod_facts.defs.push((child.end_byte(), "format"));
                        }
                    }
                } else if child.kind() == "use_declaration" {
                    if let Some(argument) = child.child_by_field_name("argument") {
                        let mut imports = Vec::new();
                        super::collect_use(argument, source, "", &mut imports);
                        for (path, alias) in imports {
                            if (alias == "vec" || alias == "format")
                                && !path.starts_with("std::")
                                && !path.starts_with("core::")
                                && !path.starts_with("alloc::")
                                && is_imported_target_macro(node, &path, source, file_root)
                            {
                                mod_facts.imports.insert(if alias == "vec" { "vec" } else { "format" });
                            }
                        }
                    }
                } else {
                    stack.push(child);
                }
            }
            facts.modules.insert(node.id(), mod_facts);
        } else if kind == "block" {
            let mut block_facts = BlockMacroFacts::default();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "macro_definition" {
                    if let Some(m_name) = child.child_by_field_name("name") {
                        let name = text(m_name, source).trim();
                        if name == "vec" {
                            block_facts.defs.push((child.end_byte(), "vec"));
                        } else if name == "format" {
                            block_facts.defs.push((child.end_byte(), "format"));
                        }
                    }
                } else if child.kind() == "use_declaration" {
                    if let Some(argument) = child.child_by_field_name("argument") {
                        let mut imports = Vec::new();
                        super::collect_use(argument, source, "", &mut imports);
                        for (path, alias) in imports {
                            if (alias == "vec" || alias == "format")
                                && !path.starts_with("std::")
                                && !path.starts_with("core::")
                                && !path.starts_with("alloc::")
                                && is_imported_target_macro(node, &path, source, file_root)
                            {
                                block_facts.imports.insert(if alias == "vec" { "vec" } else { "format" });
                            }
                        }
                    }
                } else {
                    stack.push(child);
                }
            }
            facts.blocks.insert(node.id(), block_facts);
        } else {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                stack.push(child);
            }
        }
    }

    facts
}

std::thread_local! {
    static CACHED_FILE_FACTS: std::cell::RefCell<Option<(usize, std::sync::Arc<FileMacroFacts>)>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn clear_file_macro_facts() {
    CACHED_FILE_FACTS.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

pub(crate) fn file_macro_facts(root: Node<'_>, source: &str) -> std::sync::Arc<FileMacroFacts> {
    let file_root = get_file_root(root);
    let root_id = file_root.id();
    CACHED_FILE_FACTS.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some((cached_id, ref facts)) = *borrow {
            if cached_id == root_id {
                return facts.clone();
            }
        }
        let facts = std::sync::Arc::new(build_file_macro_facts(file_root, source));
        *borrow = Some((root_id, facts.clone()));
        facts
    })
}

fn collect_ancestor_shadows(
    node: Node<'_>,
    _source: &str,
    file_facts: &FileMacroFacts,
) -> HashSet<&'static str> {
    let mut shadows = file_facts.root_macro_imports.clone();
    for (end_byte, name) in &file_facts.root_macro_defs {
        if *end_byte <= node.start_byte() {
            shadows.insert(name);
        }
    }

    let file_root = get_file_root(node);
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        if parent == file_root {
            break;
        }
        if parent.kind() == "declaration_list" {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "mod_item" {
                    if let Some(mod_facts) = file_facts.modules.get(&parent.id()) {
                        shadows.extend(&mod_facts.imports);
                        for (end_byte, name) in &mod_facts.defs {
                            if *end_byte <= node.start_byte() {
                                shadows.insert(name);
                            }
                        }
                    }
                }
            }
        } else if parent.kind() == "block" {
            if let Some(block_facts) = file_facts.blocks.get(&parent.id()) {
                shadows.extend(&block_facts.imports);
                for (end_byte, name) in &block_facts.defs {
                    if *end_byte <= node.start_byte() {
                        shadows.insert(name);
                    }
                }
            }
        }
        curr = parent;
    }

    shadows
}
