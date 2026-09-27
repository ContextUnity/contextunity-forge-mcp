use super::*;
pub struct Php;
pub static PHP: Php = Php;
impl LanguageProfile for Php {
    fn id(&self) -> &'static str {
        "php"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("php")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_php::language_php()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "class_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "trait_declaration" => Some("trait"),
            "enum_declaration" => Some("enum"),
            "function_definition" => Some("function"),
            "method_declaration" => Some("method"),
            "namespace_definition" => Some("namespace"),
            _ => None,
        }
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "php"
    }
    fn normalize_import(&self, _owner: &str, _module: &str) -> Option<ImportPath> {
        None
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        name == "this" && owner.details["receiver_name"] == "this"
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        _source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        SymbolMetadata {
            receiver_name: (node.kind() == "method_declaration"
                && syntax::child(node, &["static_modifier"]).is_none())
            .then(|| "this".into()),
            ..Default::default()
        }
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        syntax::bindings(
            self,
            node,
            source,
            &[
                "simple_parameter",
                "variadic_parameter",
                "property_promotion_parameter",
            ],
            &["assignment_expression"],
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
        if matches!(
            ctx.node.kind(),
            "namespace_use_clause" | "namespace_use_group_clause"
        ) {
            let target = syntax::named(
                ctx.node,
                ctx.source,
                &["qualified_name", "namespace_name", "name"],
            );
            if let Some(target) = target {
                let alias = syntax::child(ctx.node, &["namespace_aliasing_clause"])
                    .and_then(|n| syntax::named(n, ctx.source, &["name"]))
                    .or_else(|| target.rsplit('\\').next());
                syntax::import(ctx, facts, target, alias.map(str::to_owned));
            }
        }
        if matches!(
            ctx.node.kind(),
            "include_expression"
                | "include_once_expression"
                | "require_expression"
                | "require_once_expression"
        ) {
            syntax::import(ctx, facts, text(ctx.node, ctx.source), None);
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        match ctx.node.kind() {
            "function_call_expression" => {
                if let Some(callee) = ctx.node.child_by_field_name("function") {
                    syntax::call_reference(
                        ctx,
                        facts,
                        text(callee, ctx.source),
                        callee.kind() == "variable_name",
                    );
                }
            }
            "member_call_expression"
            | "nullsafe_member_call_expression"
            | "scoped_call_expression" => {
                if let (Some(name), Some(receiver)) = (
                    ctx.node.child_by_field_name("name"),
                    ctx.node
                        .child_by_field_name("object")
                        .or_else(|| ctx.node.child_by_field_name("scope")),
                ) {
                    let proven_this = text(receiver, ctx.source) == "$this"
                        && facts
                            .nodes
                            .iter()
                            .find(|n| n.id == ctx.owner)
                            .is_some_and(|n| {
                                n.kind == "method" && n.details["receiver_name"] == "this"
                            });
                    let expression = format!(
                        "{}.{}",
                        if proven_this {
                            "this"
                        } else {
                            text(receiver, ctx.source)
                        },
                        text(name, ctx.source)
                    );
                    syntax::call_reference(
                        ctx,
                        facts,
                        &expression,
                        name.kind() == "variable_name"
                            || (receiver.kind() == "variable_name" && !proven_this),
                    );
                }
            }
            "object_creation_expression" => {
                if let Some(name) = ctx.node.named_child(0) {
                    syntax::call_reference(
                        ctx,
                        facts,
                        text(name, ctx.source),
                        name.kind() == "variable_name",
                    );
                }
            }
            _ => {}
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(base) = syntax::child(ctx.node, &["base_clause"]) {
            syntax::types(ctx, facts, base, "inherits", &[]);
        }
        if let Some(base) = syntax::child(ctx.node, &["class_interface_clause"]) {
            syntax::types(ctx, facts, base, "implements", &[]);
        }
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&PHP];
