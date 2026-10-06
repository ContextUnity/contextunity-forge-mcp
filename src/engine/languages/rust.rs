use super::*;
use crate::core::models::ReceiverHint;
use crate::engine::ast::relations;
#[path = "rust/linker.rs"]
pub(crate) mod linker;
#[path = "rust/patterns.rs"]
pub(crate) mod patterns;
#[path = "rust/value_flow.rs"]
mod value_flow;
/// Performs language.
pub fn language() -> tree_sitter::Language {
    tree_sitter_rust::language()
}
/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_item" => Some("function"),
        "function_signature_item" => Some("function"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        "trait_item" => Some("trait"),
        "type_item" => Some("type"),
        "impl_item" => Some("impl"),
        "mod_item" => Some("module_declaration"),
        "macro_definition" => Some("macro"),
        _ => None,
    }
}

fn method_owner(node: Syntax<'_>) -> Option<Syntax<'_>> {
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        match ancestor.kind() {
            "declaration_list" => parent = ancestor.parent(),
            "impl_item" | "trait_item" => return Some(ancestor),
            _ => return None,
        }
    }
    None
}

fn has_unproven_bounds(node: Syntax<'_>, source: &str) -> bool {
    node.named_children(&mut node.walk())
        .any(|child| child.kind() == "where_clause")
        || node
            .child_by_field_name("type_parameters")
            .is_some_and(|parameters| {
                parameters
                    .named_children(&mut parameters.walk())
                    .any(|parameter| text(parameter, source).contains([':', '=']))
            })
}

fn uniform_generic_impl(node: Syntax<'_>, source: &str) -> bool {
    let (Some(parameters), Some(receiver)) = (
        node.child_by_field_name("type_parameters"),
        node.child_by_field_name("type"),
    ) else {
        return false;
    };
    if receiver.kind() != "generic_type" || has_unproven_bounds(node, source) {
        return false;
    }
    let Some(arguments) = receiver.child_by_field_name("type_arguments") else {
        return false;
    };
    let mut names: Vec<_> = parameters
        .named_children(&mut parameters.walk())
        .map(|parameter| text(parameter, source))
        .collect();
    if names.is_empty()
        || names.iter().any(|name| {
            !name.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
                || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
    {
        return false;
    }
    for argument in arguments.named_children(&mut arguments.walk()) {
        if argument.kind() != "type_identifier" {
            return false;
        }
        let Some(index) = names.iter().position(|name| *name == text(argument, source)) else {
            return false;
        };
        names.swap_remove(index);
    }
    names.is_empty()
}

fn call_result_hint(node: Syntax<'_>, source: &str) -> Option<ReceiverHint> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let receiver = function.child_by_field_name("value")?;
    if receiver.kind() != "call_expression" {
        return None;
    }
    let callee = receiver
        .child_by_field_name("function")
        .and_then(|node| value_flow::callee_name(node, source))?;
    if !callee.contains("::") {
        return None;
    }
    let member = function.child_by_field_name("field")?;
    if member.kind() != "field_identifier" {
        return None;
    }
    Some(ReceiverHint::CallResult {
        callee: callee.into_owned(),
        member: text(member, source).to_owned(),
    })
}

/// Represents rust data.
pub struct Rust;
/// Shared rust language profile.
pub static RUST: Rust = Rust;
impl LanguageProfile for Rust {
    fn id(&self) -> &'static str {
        "rust"
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &["Cargo.toml"]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        if filename != "Cargo.toml" {
            return Vec::new();
        }
        let mut dependencies = Vec::new();
        super::toml_manifest::visit_toml_pairs(content, &mut |path, _value| {
            let parts: Vec<_> = path.split('.').collect();
            let is_dependency_section = |section: &str| {
                matches!(
                    section,
                    "dependencies" | "dev-dependencies" | "build-dependencies"
                )
            };
            let dependency_name = match parts.as_slice() {
                [section, name, ..] if is_dependency_section(section) => Some(*name),
                ["workspace", "dependencies", name, ..] => Some(*name),
                ["target", _, section, name, ..] if is_dependency_section(section) => Some(*name),
                _ => None,
            };
            if let Some(name) = dependency_name {
                let name = name.replace('-', "_");
                if !name.is_empty() {
                    dependencies.push(name);
                }
            }
        });
        dependencies.sort();
        dependencies.dedup();
        dependencies
    }
    fn is_stdlib(&self, module: &str) -> bool {
        matches!(
            module.split("::").next(),
            Some("std" | "core" | "alloc" | "proc_macro")
        )
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("rust")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        if matches!(node.kind(), "function_item" | "function_signature_item")
            && method_owner(node).is_some()
        {
            Some("method")
        } else {
            self.symbol_kind(node.kind())
        }
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        if kind == "type" {
            "type"
        } else if kind == "macro" {
            "macro"
        } else {
            "rs"
        }
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path).trim_end_matches("/mod").replace('/', ".")
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        parse_file(self, path, source, module, facts)
    }
    fn value_flow(&self, node: Syntax<'_>, source: &str) -> crate::core::semantic::ValueFlowFacts {
        value_flow::extract(node, source)
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "use_declaration" {
            if let Some(argument) = ctx.node.child_by_field_name("argument") {
                let mut imports = Vec::new();
                collect_use(argument, ctx.source, "", &mut imports);
                for (name, alias) in imports {
                    ctx.import(facts, name.clone(), Some(alias), Some(name));
                }
            }
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        match module.split("::").next()? {
            "std" | "core" | "alloc" => Some("Rust standard library"),
            _ => None,
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "call_expression" {
            if let Some(callee) = ctx
                .node
                .child_by_field_name("function")
                .and_then(|function| value_flow::callee_name(function, ctx.source))
            {
                facts.references.push(Reference {
                    source: ctx.owner.into(),
                    expression: callee.into_owned(),
                    kind: "calls".into(),
                    line: ctx.line(),
                    column: ctx.node.start_position().column,
                    alias: None,
                    module: None,
                    dynamic: false,
                    receiver_hint: None,
                });
            } else {
                let hint = call_result_hint(ctx.node, ctx.source);
                let previous = facts.references.len();
                call(ctx, facts, false);
                if let (Some(hint), Some(reference)) = (hint, facts.references.get_mut(previous)) {
                    reference.receiver_hint = Some(hint);
                }
            }
        } else if ctx.node.kind() == "macro_invocation" {
            call(ctx, facts, false);
        } else if ctx.node.kind() == "struct_expression" {
            if let Some(name) = ctx.node.child_by_field_name("name") {
                let text = relations::type_name(name, ctx.source);
                let clean = text.split('<').next().unwrap_or(text).trim();
                relations::reference(facts, ctx.owner, clean, "references", ctx.line());
            }
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        let (receiver, member) = if let Some((r, m)) = symbol.split_once("::") {
            (r, m)
        } else if let Some((r, m)) = symbol.split_once('.') {
            (r, m)
        } else {
            (symbol, "")
        };
        if matches!(receiver, "Self" | "Path" | "PathBuf" | "Rc" | "Arc" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet") {
            return false;
        }
        if !member.is_empty() {
            return self.builtin_member(receiver, member);
        }
        matches!(
            symbol,
            "Ok" | "Err"
                | "Some"
                | "None"
                | "vec"
                | "println"
                | "eprintln"
                | "print"
                | "eprint"
                | "format"
                | "panic"
                | "assert"
                | "assert_eq"
                | "assert_ne"
                | "matches"
                | "write"
                | "writeln"
                | "todo"
                | "unimplemented"
                | "unreachable"
                | "drop"
                | "str"
                | "bool"
                | "char"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "f32"
                | "f64"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
                | "Box"
                | "Drop"
                | "Clone"
                | "Default"
        )
    }
    fn builtin_type(&self, name: &str) -> bool {
        matches!(
            name,
            "str"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
                | "Box"
                | "Rc"
                | "Arc"
                | "Path"
                | "PathBuf"
                | "HashMap"
                | "HashSet"
                | "BTreeMap"
                | "BTreeSet"
        )
    }

    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        match receiver {
            "str" => matches!(
                member,
                "len"
                    | "is_empty"
                    | "as_bytes"
                    | "chars"
                    | "char_indices"
                    | "bytes"
                    | "trim"
                    | "trim_start"
                    | "trim_end"
                    | "split"
                    | "split_once"
                    | "rsplit_once"
                    | "split_whitespace"
                    | "contains"
                    | "starts_with"
                    | "ends_with"
                    | "is_ascii"
                    | "eq_ignore_ascii_case"
            ),
            "String" => matches!(
                member,
                "new"
                    | "from"
                    | "with_capacity"
                    | "default"
                    | "clone"
                    | "as_ref"
                    | "as_mut"
                    | "len"
                    | "is_empty"
                    | "capacity"
                    | "clear"
                    | "push"
                    | "push_str"
                    | "as_str"
                    | "as_bytes"
                    | "chars"
                    | "bytes"
                    | "lines"
                    | "into_bytes"
                    | "to_string"
                    | "trim"
                    | "split"
                    | "contains"
                    | "starts_with"
                    | "ends_with"
            ),
            "Vec" => matches!(
                member,
                "new"
                    | "from"
                    | "with_capacity"
                    | "default"
                    | "clone"
                    | "as_ref"
                    | "as_mut"
                    | "len"
                    | "is_empty"
                    | "capacity"
                    | "clear"
                    | "push"
                    | "pop"
                    | "get"
                    | "get_mut"
                    | "first"
                    | "last"
                    | "iter"
                    | "iter_mut"
                    | "into_iter"
                    | "extend"
                    | "drain"
                    | "as_slice"
                    | "as_mut_slice"
                    | "contains"
                    | "sort"
                    | "sort_unstable"
                    | "retain"
                    | "truncate"
            ),
            "Path" | "PathBuf" => matches!(
                member,
                "new" | "from" | "join" | "parent" | "file_name" | "file_stem" | "extension" | "exists" | "is_file" | "is_dir" | "to_str" | "to_string_lossy" | "display" | "as_path"
            ),
            "Box" => matches!(
                member,
                "new" | "from" | "clone" | "default" | "as_ref" | "as_mut"
            ),
            "Rc" | "Arc" => matches!(
                member,
                "new" | "from" | "clone" | "default" | "as_ref"
            ),
            "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" => matches!(
                member,
                "new" | "with_capacity" | "default" | "insert" | "get" | "get_mut" | "contains_key" | "contains" | "remove" | "iter" | "keys" | "values" | "len" | "is_empty" | "clear" | "entry"
            ),
            "Option" => matches!(
                member,
                "default"
                    | "from"
                    | "clone"
                    | "is_some"
                    | "is_none"
                    | "unwrap"
                    | "expect"
                    | "map"
                    | "and_then"
                    | "unwrap_or"
                    | "unwrap_or_else"
                    | "unwrap_or_default"
                    | "ok_or"
                    | "ok_or_else"
                    | "cloned"
                    | "copied"
                    | "as_ref"
                    | "as_mut"
                    | "take"
                    | "replace"
            ),
            "Result" => matches!(
                member,
                "clone"
                    | "is_ok"
                    | "is_err"
                    | "unwrap"
                    | "expect"
                    | "unwrap_err"
                    | "expect_err"
                    | "map"
                    | "map_err"
                    | "unwrap_or"
                    | "unwrap_or_else"
                    | "unwrap_or_default"
                    | "ok"
                    | "err"
                    | "as_ref"
                    | "as_mut"
            ),
            _ => false,
        }
    }
    fn builtin_generic(&self, receiver: &str) -> bool {
        matches!(receiver, "Vec" | "Option" | "Result" | "Box" | "Rc" | "Arc" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet")
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        let (_, local) = workspace_path(owner);
        let (namespace, relative) = if let Some(tail) = module.strip_prefix("crate::") {
            let crate_prefix = if let Some((before_src, _)) = local.split_once("/src/") {
                format!("{}.src", before_src.replace('/', "."))
            } else if local.starts_with("src/") {
                "src".to_string()
            } else {
                local
                    .split_once('/')
                    .map_or("src", |(first, _)| first)
                    .replace('/', ".")
            };
            let ns = if tail == "*" {
                crate_prefix
            } else {
                format!("{crate_prefix}.{}", tail.replace("::", "."))
            };
            (ns, true)
        } else if let Some(tail) = module.strip_prefix("self::") {
            let ns = if tail == "*" {
                self.module_name(local)
            } else {
                format!("{}.{}", self.module_name(local), tail.replace("::", "."))
            };
            (ns, true)
        } else if module.starts_with("super::") {
            let mut base = self
                .module_name(local)
                .split('.')
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut tail = module;
            while let Some(rest) = tail.strip_prefix("super::") {
                base.pop()?;
                tail = rest;
            }
            if tail != "*" {
                base.push(tail.replace("::", "."));
            }
            (base.join("."), true)
        } else {
            let (first, tail) = module.split_once("::").unwrap_or((module, ""));
            let mut resolved_crate = None;
            for prefix in ["crates", "packages", "libs", "modules", ""] {
                let candidate = if prefix.is_empty() {
                    std::path::PathBuf::from(first).join("src")
                } else {
                    std::path::PathBuf::from(prefix).join(first).join("src")
                };
                if candidate.exists() {
                    let mod_prefix = candidate
                        .to_string_lossy()
                        .replace('\\', "/")
                        .replace('/', ".");
                    resolved_crate = Some(mod_prefix);
                    break;
                }
            }
            if let Some(crate_mod) = resolved_crate {
                let full = if tail.is_empty() {
                    crate_mod
                } else {
                    format!("{crate_mod}.{}", tail.replace("::", "."))
                };
                (full, false)
            } else {
                (module.replace("::", "."), false)
            }
        };
        Some(ImportPath {
            namespace,
            relative,
            symbol_path: true,
        })
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "impl_item" {
            if let Some(owner) = facts.nodes.iter_mut().find(|node| node.id == ctx.owner) {
                owner.details["generic_impl"] =
                    serde_json::json!(ctx.node.child_by_field_name("type_parameters").is_some());
                if uniform_generic_impl(ctx.node, ctx.source) {
                    owner.details["uniform_generic_impl"] = serde_json::json!(true);
                }
                if let Some(trait_name) = field(ctx.node, ctx.source, "trait") {
                    owner.details["impl_trait"] = serde_json::json!(trait_name);
                }
                if let Some(receiver) = field(ctx.node, ctx.source, "type") {
                    owner.details["receiver_type"] = serde_json::json!(receiver);
                }
            }
            if let Some(tr) = ctx.node.child_by_field_name("trait") {
                relations::reference(
                    facts,
                    ctx.owner,
                    relations::type_name(tr, ctx.source),
                    "implements",
                    ctx.line(),
                );
            }
        } else if matches!(ctx.node.kind(), "function_item" | "function_signature_item") {
            if has_unproven_bounds(ctx.node, ctx.source) {
                if let Some(method) = facts.nodes.iter_mut().find(|node| node.id == ctx.owner) {
                    method.details["unproven_bounds"] = serde_json::json!(true);
                }
            }
            if ctx.node.kind() == "function_item"
                && method_owner(ctx.node).is_some_and(|owner| owner.kind() == "trait_item")
            {
                if let Some(method) = facts.nodes.iter_mut().find(|node| node.id == ctx.owner) {
                    method.details["trait_default"] = serde_json::json!(true);
                }
            }
            if let Some(parameters) = ctx.node.child_by_field_name("parameters") {
                let mut c = parameters.walk();
                for param in parameters.named_children(&mut c) {
                    if let Some(ty) = param.child_by_field_name("type") {
                        value_flow::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
                    }
                }
            }
            if let Some(ret) = ctx.node.child_by_field_name("return_type") {
                value_flow::type_references(facts, ctx.owner, ret, ctx.source, ctx.line());
            }
        } else if ctx.node.kind() == "type_item" {
            if let Some(ty) = ctx.node.child_by_field_name("type") {
                value_flow::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
            }
        }
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        let mut param_types = BTreeMap::new();
        if matches!(node.kind(), "function_item" | "function_signature_item") {
            if let Some(parameters) = node.child_by_field_name("parameters") {
                let mut cursor = parameters.walk();
                for parameter in parameters.named_children(&mut cursor) {
                    let Some(pattern) = parameter.child_by_field_name("pattern") else {
                        continue;
                    };
                    let Some(ty) = parameter.child_by_field_name("type") else {
                        continue;
                    };
                    let pattern_text = text(pattern, source).trim();
                    let name = pattern_text
                        .strip_prefix("mut ")
                        .or_else(|| pattern_text.strip_prefix("ref "))
                        .unwrap_or(pattern_text)
                        .trim();
                    if !matches!(
                        value_flow::type_expr(ty, source),
                        crate::core::semantic::TypeExpr::Unknown
                    ) && name
                        .chars()
                        .all(|character| character.is_alphanumeric() || character == '_')
                        && !name.is_empty()
                    {
                        param_types.insert(name.to_owned(), text(ty, source).trim().to_owned());
                    }
                }
            }
        }
        let Some(owner) = method_owner(node) else {
            return SymbolMetadata {
                param_types,
                is_async: value_flow::is_async(node, source),
                ..Default::default()
            };
        };
        let receiver_type = if owner.kind() == "impl_item" {
            owner
                .child_by_field_name("type")
                .map(|receiver| text(receiver, source).trim().to_owned())
        } else {
            Some("Self".into())
        };
        let has_receiver = node
            .child_by_field_name("parameters")
            .is_some_and(|parameters| {
                let mut cursor = parameters.walk();
                let has_receiver = parameters
                    .named_children(&mut cursor)
                    .any(|parameter| parameter.kind() == "self_parameter");
                has_receiver
            });
        SymbolMetadata {
            param_types,
            is_async: value_flow::is_async(node, source),
            receiver_name: has_receiver.then(|| "self".into()),
            receiver_type,
            is_method: Some(true),
            is_static: Some(!has_receiver),
            ..Default::default()
        }
    }
    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        relations::mutation(
            ctx,
            facts,
            &["field_declaration"],
            &["compound_assignment_expr", "assignment_expr"],
            &["field_expression"],
        );
        relations::member_access(ctx, facts, &["field_expression"]);
    }
    fn test_attribute(&self, node: Syntax<'_>, source: &str) -> bool {
        let mut previous = node.prev_named_sibling();
        while let Some(attr) = previous.filter(|n| n.kind() == "attribute_item") {
            let value = text(attr, source).replace(' ', "");
            if value == "#[test]"
                || value.starts_with("#[tokio::test")
                || value.starts_with("#[async_std::test")
            {
                return true;
            }
            previous = attr.prev_named_sibling();
        }
        false
    }
}

fn collect_use(node: Syntax<'_>, source: &str, prefix: &str, out: &mut Vec<(String, String)>) {
    fn joined(prefix: &str, name: &str) -> String {
        if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}::{name}")
        }
    }
    match node.kind() {
        "scoped_use_list" => {
            if let (Some(path), Some(list)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("list"),
            ) {
                collect_use(list, source, &joined(prefix, text(path, source)), out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_use(child, source, prefix, out);
            }
        }
        "use_as_clause" => {
            if let (Some(path), Some(alias)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias"),
            ) {
                let path = text(path, source);
                let name = if path == "self" {
                    prefix.to_owned()
                } else {
                    joined(prefix, path)
                };
                if !name.is_empty() {
                    out.push((name, text(alias, source).to_owned()));
                }
            }
        }
        "use_wildcard" => {
            let path = node.named_child(0).map(|child| text(child, source));
            let name = path.map_or_else(|| prefix.to_owned(), |path| joined(prefix, path));
            out.push((format!("{name}::*"), "*".into()));
        }
        "self" if !prefix.is_empty() => {
            out.push((
                prefix.to_owned(),
                prefix.rsplit("::").next().unwrap_or(prefix).into(),
            ));
        }
        "identifier" | "scoped_identifier" | "crate" | "super" | "self" => {
            let name = joined(prefix, text(node, source));
            out.push((
                name.clone(),
                name.rsplit("::").next().unwrap_or(&name).into(),
            ));
        }
        _ => {}
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&RUST];
