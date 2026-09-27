use super::*;
pub struct Kotlin;
pub static KOTLIN: Kotlin = Kotlin;
impl LanguageProfile for Kotlin {
    fn id(&self) -> &'static str {
        "kotlin"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("kotlin")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["kt", "kts"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_kotlin::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "class_declaration" | "object_declaration" => Some("class"),
            "function_declaration" => Some("function"),
            "type_alias" => Some("type"),
            _ => None,
        }
    }
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        if node.kind() == "class_declaration" {
            let mut c = node.walk();
            if node.children(&mut c).any(|n| n.kind() == "interface") {
                return Some("interface");
            }
        }
        self.symbol_kind(node.kind())
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        syntax::named(node, source, &["simple_identifier", "type_identifier"])
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "kt"
    }
    fn normalize_import(&self, _owner: &str, _module: &str) -> Option<ImportPath> {
        None
    }
    fn class_scope(&self) -> bool {
        true
    }
    fn receiver(&self, name: &str, _owner: &Node) -> bool {
        name == "this"
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        syntax::bindings(
            self,
            node,
            source,
            &["parameter", "class_parameter"],
            &["variable_declaration"],
        )
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
        if ctx.node.kind() == "import_header" {
            if let Some(module) = syntax::named(ctx.node, ctx.source, &["identifier"]) {
                let alias = syntax::child(ctx.node, &["import_alias"])
                    .and_then(|n| syntax::named(n, ctx.source, &["type_identifier"]))
                    .or_else(|| module.rsplit('.').next());
                syntax::import(ctx, facts, module, alias.map(str::to_owned));
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(
            ctx.node.kind(),
            "constructor_invocation" | "constructor_delegation_call"
        ) {
            syntax::call_reference(ctx, facts, text(ctx.node, ctx.source), true);
        }
        if ctx.node.kind() == "call_expression" {
            if let Some(callee) = ctx.node.named_child(0) {
                syntax::call_reference(
                    ctx,
                    facts,
                    text(callee, ctx.source),
                    !matches!(callee.kind(), "simple_identifier" | "navigation_expression"),
                );
            }
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        {
            let mut c = ctx.node.walk();
            for base in ctx
                .node
                .named_children(&mut c)
                .filter(|n| n.kind() == "delegation_specifier")
            {
                let mut node = base;
                while matches!(
                    node.kind(),
                    "delegation_specifier" | "constructor_invocation" | "explicit_delegation"
                ) {
                    let Some(child) = node.named_child(0) else {
                        break;
                    };
                    node = child;
                }
                crate::engine::ast::relations::reference(
                    facts,
                    ctx.owner,
                    text(node, ctx.source),
                    "bases",
                    ctx.line(),
                );
            }
        }
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&KOTLIN];
