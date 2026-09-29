use super::*;
use crate::engine::ast::relations;
pub fn language() -> tree_sitter::Language {
    tree_sitter_rust::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_item" => Some("function"),
        "function_signature_item" => Some("function"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        "trait_item" => Some("trait"),
        "impl_item" => Some("impl"),
        "mod_item" => Some("module_declaration"),
        "macro_definition" => Some("macro"),
        _ => None,
    }
}

pub struct Rust;
pub static RUST: Rust = Rust;
impl LanguageProfile for Rust {
    fn id(&self) -> &'static str {
        "rust"
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
    fn node_prefix(&self, kind: &str) -> &'static str {
        if kind == "macro" {
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
        if matches!(ctx.node.kind(), "call_expression" | "macro_invocation") {
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
        matches!(
            symbol,
            "Ok" | "Err" | "Some" | "None"
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
                | "Rc"
                | "Arc"
                | "HashMap"
                | "HashSet"
                | "BTreeMap"
                | "BTreeSet"
                | "Self"
        )
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        let (_, local) = workspace_path(owner);
        let (namespace, relative) = if let Some(tail) = module.strip_prefix("crate::") {
            let crate_prefix = if let Some((before_src, _)) = local.split_once("/src/") {
                format!("{}.src", before_src.replace('/', "."))
            } else if local.starts_with("src/") {
                "src".to_string()
            } else {
                local.split_once('/').map_or("src", |(first, _)| first).replace('/', ".")
            };
            (format!("{crate_prefix}.{}", tail.replace("::", ".")), true)
        } else if let Some(tail) = module.strip_prefix("self::") {
            (
                format!("{}.{}", self.module_name(local), tail.replace("::", ".")),
                true,
            )
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
            base.push(tail.replace("::", "."));
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
                    let mod_prefix = candidate.to_string_lossy().replace('\\', "/").replace('/', ".");
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
            if let Some(parameters) = ctx.node.child_by_field_name("parameters") {
                let mut c = parameters.walk();
                for param in parameters.named_children(&mut c) {
                    if let Some(ty) = param.child_by_field_name("type") {
                        relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
                    }
                }
            }
            if let Some(ret) = ctx.node.child_by_field_name("return_type") {
                relations::type_references(facts, ctx.owner, ret, ctx.source, ctx.line());
            }
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
            out.push((prefix.to_owned(), prefix.rsplit("::").next().unwrap_or(prefix).into()));
        }
        "identifier" | "scoped_identifier" | "crate" | "super" | "self" => {
            let name = joined(prefix, text(node, source));
            out.push((name.clone(), name.rsplit("::").next().unwrap_or(&name).into()));
        }
        _ => {}
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&RUST];
