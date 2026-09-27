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
        let (node, source) = (ctx.node, ctx.source);
        let statement = text(node, source);
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        if node.kind() == "use_declaration" {
            let value = field(node, source, "argument")
                .unwrap_or(statement.trim_start_matches("use ").trim_end_matches(';'));
            fn expand(value: &str, prefix: &str, out: &mut Vec<(String, String)>) {
                let trimmed = value.trim().trim_matches(';');
                if let Some((base, rest)) = trimmed.split_once('{') {
                    let root = format!("{prefix}{}", base.trim());
                    let mut depth = 0;
                    let mut current = String::new();
                    for c in rest.trim_end_matches('}').chars() {
                        match c {
                            '{' => {
                                depth += 1;
                                current.push(c);
                            }
                            '}' => {
                                if depth > 0 {
                                    depth -= 1;
                                    current.push(c);
                                }
                            }
                            ',' if depth == 0 => {
                                let item = current.trim();
                                if !item.is_empty() {
                                    expand(item, &root, out);
                                }
                                current.clear();
                            }
                            _ => current.push(c),
                        }
                    }
                    let item = current.trim();
                    if !item.is_empty() {
                        expand(item, &root, out);
                    }
                } else {
                    let clean = trimmed.trim_matches(['}', '{', ' ']);
                    if clean.is_empty() {
                        return;
                    }
                    let (name, alias) = clean
                        .split_once(" as ")
                        .unwrap_or((clean, clean.rsplit("::").next().unwrap_or(clean)));
                    out.push((format!("{prefix}{}", name.trim()), alias.trim().into()));
                }
            }
            let mut names = Vec::new();
            expand(value, "", &mut names);
            for (name, alias) in names {
                add(name.clone(), Some(alias), Some(name));
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call_expression" | "macro_invocation") {
            call(ctx, facts, false);
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

pub static PROFILES: &[&dyn LanguageProfile] = &[&RUST];
