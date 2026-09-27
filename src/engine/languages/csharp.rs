use super::*;
pub struct CSharp;
pub static CSHARP: CSharp = CSharp;
impl LanguageProfile for CSharp {
    fn id(&self) -> &'static str {
        "csharp"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("csharp")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["cs"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_c_sharp::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "class_declaration" | "record_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "struct_declaration" => Some("struct"),
            "enum_declaration" => Some("enum"),
            "method_declaration" | "constructor_declaration" => Some("method"),
            "local_function_statement" => Some("function"),
            "namespace_declaration" | "file_scoped_namespace_declaration" => Some("namespace"),
            _ => None,
        }
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "cs"
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
            &["parameter", "parameter_list"],
            &["variable_declarator"],
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
        if ctx.node.kind() == "using_directive" {
            let mut c = ctx.node.walk();
            if let Some(target) = ctx
                .node
                .named_children(&mut c)
                .find(|n| Some(*n) != ctx.node.child_by_field_name("name"))
            {
                syntax::import(
                    ctx,
                    facts,
                    text(target, ctx.source),
                    field(ctx.node, ctx.source, "name").map(str::to_owned),
                );
            };
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(
            ctx.node.kind(),
            "object_creation_expression"
                | "implicit_object_creation_expression"
                | "constructor_initializer"
        ) {
            syntax::call_reference(ctx, facts, text(ctx.node, ctx.source), true);
        }
        if ctx.node.kind() == "invocation_expression" {
            call(ctx, facts, false);
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(bases) = syntax::child(ctx.node, &["base_list"]) {
            syntax::types(ctx, facts, bases, "bases", &[]);
        }
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&CSHARP];
