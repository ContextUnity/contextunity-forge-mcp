use super::*;
pub struct Ruby;
pub static RUBY: Ruby = Ruby;
impl LanguageProfile for Ruby {
    fn id(&self) -> &'static str {
        "ruby"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("ruby")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["rb"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_ruby::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "class" => Some("class"),
            "module" => Some("namespace"),
            "method" | "singleton_method" => Some("method"),
            "lambda" => Some("function"),
            _ => None,
        }
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "rb"
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        module
            .strip_prefix("./")
            .and_then(|module| relative_namespace(owner, module.trim_end_matches(".rb")))
            .map(|namespace| ImportPath {
                namespace,
                relative: true,
                symbol_path: false,
            })
    }
    fn class_scope(&self) -> bool {
        true
    }
    fn receiver(&self, name: &str, _owner: &Node) -> bool {
        name == "self"
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        let mut bindings = syntax::bindings(
            self,
            node,
            source,
            &[
                "optional_parameter",
                "keyword_parameter",
                "splat_parameter",
                "hash_splat_parameter",
                "block_parameter",
            ],
            &["assignment"],
        );
        if let Some(params) = node.child_by_field_name("parameters") {
            let mut c = params.walk();
            bindings.all.extend(
                params
                    .named_children(&mut c)
                    .filter(|n| n.kind() == "identifier")
                    .map(|n| text(n, source).into()),
            );
        }
        bindings
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
        if ctx.node.kind() == "call" && ctx.node.child_by_field_name("receiver").is_none() {
            let method = field(ctx.node, ctx.source, "method");
            if matches!(method, Some("require" | "require_relative" | "load")) {
                let value = ctx
                    .node
                    .child_by_field_name("arguments")
                    .and_then(|n| n.named_child(0));
                let raw = value.map_or_else(|| text(ctx.node, ctx.source), |n| text(n, ctx.source));
                let literal = value
                    .is_some_and(|n| n.kind() == "string" && !text(n, ctx.source).contains("#{"));
                let path = raw.trim_matches(['\'', '"']);
                let module = if method == Some("require_relative") && literal {
                    format!("./{path}")
                } else {
                    raw.into()
                };
                syntax::import(ctx, facts, &module, None);
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(
            ctx.node.kind(),
            "yield" | "super" | "element_reference" | "binary" | "unary"
        ) || (ctx.node.kind() == "identifier" && bare_value_expression(ctx.node))
        {
            syntax::call_reference(ctx, facts, text(ctx.node, ctx.source), true);
        }
        if ctx.node.kind() == "call" {
            if let Some(name) = ctx.node.child_by_field_name("method") {
                if ctx.node.child_by_field_name("receiver").is_none()
                    && matches!(
                        text(name, ctx.source),
                        "require" | "require_relative" | "load"
                    )
                {
                    return;
                }
                syntax::member_call(ctx, facts, name, ctx.node.child_by_field_name("receiver"));
            }
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(base) = ctx.node.child_by_field_name("superclass") {
            syntax::types(ctx, facts, base, "inherits", &[]);
        }
    }
}

fn bare_value_expression(mut node: Syntax<'_>) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "call" if parent.child_by_field_name("method") == Some(node) => return false,
            "method" | "singleton_method" if parent.child_by_field_name("name") == Some(node) => {
                return false
            }
            "method_parameters"
            | "lambda_parameters"
            | "block_parameters"
            | "destructured_parameter"
            | "splat_parameter"
            | "hash_splat_parameter"
            | "block_parameter"
            | "exception_variable"
            | "left_assignment_list"
            | "destructured_left_assignment"
            | "rest_assignment"
            | "alias"
            | "undef" => return false,
            "optional_parameter" | "keyword_parameter" => {
                return parent.child_by_field_name("value") == Some(node)
            }
            "assignment" | "operator_assignment" => {
                return parent.child_by_field_name("right") == Some(node)
            }
            "for" if parent.child_by_field_name("pattern") == Some(node) => return false,
            "rescue" if parent.child_by_field_name("variable") == Some(node) => return false,
            _ => {}
        }
        node = parent;
    }
    true
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&RUBY];
