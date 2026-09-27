use super::*;
use crate::engine::ast::{relations, routes};
pub fn language() -> tree_sitter::Language {
    tree_sitter_python::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" | "lambda" => Some("function"),
        "class_definition" => Some("class"),
        _ => None,
    }
}

pub struct Python;
pub static PYTHON: Python = Python;
impl LanguageProfile for Python {
    fn id(&self) -> &'static str {
        "python"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["py", "pyi"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("python")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        if kind == "class" {
            "class"
        } else {
            "py"
        }
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path)
            .trim_end_matches("/__init__")
            .replace('/', ".")
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
        match node.kind() {
            "import_from_statement" => {
                let module = field(node, source, "module_name").unwrap_or("").to_owned();
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.into())),
                        Some(module.clone()),
                    );
                }
                if statement.contains('*') {
                    add("*".into(), None, Some(module));
                }
            }
            "import_statement"
                if statement.starts_with("import ")
                    && !statement.contains("from ")
                    && !statement.contains('"')
                    && !statement.contains('\'') =>
            {
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.split('.').next().unwrap_or(name).into())),
                        Some(name.into()),
                    );
                }
            }
            _ => {}
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call") {
            call(ctx, facts, false);
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        matches!(
            symbol,
            "print"
                | "len"
                | "isinstance"
                | "issubclass"
                | "str"
                | "int"
                | "float"
                | "bool"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "super"
                | "getattr"
                | "setattr"
                | "hasattr"
                | "delattr"
                | "type"
                | "repr"
                | "open"
                | "iter"
                | "next"
                | "any"
                | "all"
                | "min"
                | "max"
                | "sum"
                | "enumerate"
                | "zip"
                | "sorted"
                | "reversed"
                | "abs"
                | "round"
                | "id"
                | "hash"
                | "callable"
                | "dir"
                | "vars"
                | "help"
                | "range"
        )
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        if module.starts_with('.') {
            let count = module.bytes().take_while(|b| *b == b'.').count();
            let path = format!(
                "{}{}",
                "../".repeat(count - 1),
                module[count..].replace('.', "/")
            );
            Some(ImportPath {
                namespace: relative_namespace(owner, &path)?,
                relative: true,
                symbol_path: false,
            })
        } else {
            Some(ImportPath::absolute(module.into()))
        }
    }
    fn doc_comment(&self, node: Syntax<'_>, source: &str) -> String {
        if let Some(first) = node
            .child_by_field_name("body")
            .and_then(|b| b.named_child(0))
        {
            if first.kind() == "expression_statement"
                && first.named_child(0).is_some_and(|n| n.kind() == "string")
            {
                return text(first, source).into();
            }
        }
        ast::doc_comment(node, source)
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        matches!(name, "self" | "cls") && owner.details["receiver_name"] == name
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        let decorators = node
            .parent()
            .filter(|p| p.kind() == "decorated_definition")
            .map(|p| {
                let mut c = p.walk();
                p.named_children(&mut c)
                    .filter(|n| n.kind() == "decorator")
                    .map(|n| text(n, source).to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let receiver_name = if !decorators.iter().any(|d| d.contains("staticmethod")) {
            node.child_by_field_name("parameters")
                .and_then(|p| p.named_child(0))
                .and_then(|p| {
                    if p.kind() == "identifier" {
                        Some(text(p, source))
                    } else {
                        p.named_child(0)
                            .filter(|n| n.kind() == "identifier")
                            .map(|n| text(n, source))
                    }
                })
                .map(str::to_owned)
        } else {
            None
        };
        SymbolMetadata {
            receiver_name,
            decorators,
            bases: field(node, source, "superclasses").map(str::to_owned),
            is_async: text(node, source).trim_start().starts_with("async "),
            ..Default::default()
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(bases) = ctx.node.child_by_field_name("superclasses") {
            let mut c = bases.walk();
            for base in bases
                .named_children(&mut c)
                .filter(|n| n.kind() != "keyword_argument")
            {
                relations::reference(
                    facts,
                    ctx.owner,
                    relations::type_name(base, ctx.source),
                    "inherits",
                    ctx.line(),
                );
            }
        }
        relations::decorator_references(ctx, facts);
        routes::declaration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);
    }
    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        relations::mutation(
            ctx,
            facts,
            &[],
            &["assignment", "augmented_assignment"],
            &["attribute"],
        );
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if ctx.node.kind() == "call" {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
        }
    }
    fn finish(&self, facts: &mut Facts) {
        relations::implicit_fields(facts);
    }
    fn prepare_pattern(&self, pattern: &mut String) -> bool {
        let partial = pattern.trim_end().ends_with(':');
        if partial {
            pattern.push_str("\n    __FORGE_META_BODY\n");
        }
        partial
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&PYTHON];
