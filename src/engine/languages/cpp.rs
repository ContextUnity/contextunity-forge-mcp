use super::*;
pub struct Cpp;
pub static CPP: Cpp = Cpp;
impl LanguageProfile for Cpp {
    fn id(&self) -> &'static str {
        "cpp"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("cpp")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_cpp::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "class_specifier" => Some("class"),
            "namespace_definition" => Some("namespace"),
            _ => c_family::symbol_kind(kind),
        }
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        c_family::symbol_name(node, source)
    }
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        if matches!(node.kind(), "class_specifier" | "namespace_definition") {
            self.symbol_kind(node.kind())
        } else {
            c_family::symbol(node)
        }
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "cpp"
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        c_family::include_path(self, owner, module)
    }
    fn class_scope(&self) -> bool {
        true
    }
    fn receiver(&self, name: &str, _owner: &Node) -> bool {
        name == "this"
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        let mut bindings = c_family::bindings(self, node, source);
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if current != node && self.symbol(current).is_some() {
                continue;
            }
            if current.kind() == "expression_statement" {
                if let Some(names) = expression_declaration(current) {
                    for name in names {
                        let name = text(name, source).to_owned();
                        bindings.all.push(name.clone());
                        bindings.rebindings.push(name);
                    }
                }
            }
            let mut cursor = current.walk();
            stack.extend(current.named_children(&mut cursor));
        }
        bindings.all.sort();
        bindings.all.dedup();
        bindings.rebindings.sort();
        bindings.rebindings.dedup();
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
        c_family::include(ctx, facts);
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let class_initialization = ctx.node.kind() == "declaration"
            && ctx.node.child_by_field_name("type").is_some_and(|n| {
                matches!(
                    n.kind(),
                    "type_identifier" | "qualified_identifier" | "template_type"
                )
            });
        if class_initialization
            || matches!(
                ctx.node.kind(),
                "compound_literal_expression" | "subscript_expression"
            )
        {
            syntax::call_reference(ctx, facts, text(ctx.node, ctx.source), true);
        }
        if ctx.node.kind() == "new_expression" {
            syntax::call_reference(ctx, facts, text(ctx.node, ctx.source), true);
        }
        if ctx.node.kind() == "call_expression" {
            call(ctx, facts, false);
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(bases) = syntax::child(ctx.node, &["base_class_clause"]) {
            syntax::types(
                ctx,
                facts,
                bases,
                "inherits",
                &["access_specifier", "virtual"],
            );
        }
    }
}

fn expression_declaration(statement: Syntax<'_>) -> Option<Vec<Syntax<'_>>> {
    let mut expressions = Vec::new();
    let mut stack = vec![statement.named_child(0)?];
    while let Some(node) = stack.pop() {
        if node.kind() == "comma_expression" {
            stack.push(node.child_by_field_name("right")?);
            stack.push(node.child_by_field_name("left")?);
        } else {
            expressions.push(node);
        }
    }
    expressions
        .into_iter()
        .enumerate()
        .map(|(index, node)| expression_pointer(node, index == 0))
        .collect()
}

fn expression_pointer(mut node: Syntax<'_>, first: bool) -> Option<Syntax<'_>> {
    if node.kind() == "assignment_expression" {
        if node.child_by_field_name("operator")?.kind() != "=" {
            return None;
        }
        node = node.child_by_field_name("left")?;
    }
    if node.kind() != "call_expression"
        || node.child_by_field_name("arguments")?.named_child_count() != 0
    {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    let pointer = if first {
        if function.kind() != "call_expression"
            || function.child_by_field_name("function")?.kind() != "primitive_type"
        {
            return None;
        }
        let arguments = function.child_by_field_name("arguments")?;
        if arguments.named_child_count() != 1 {
            return None;
        }
        arguments.named_child(0)?
    } else {
        function
    };
    let pointer = unparenthesized(pointer)?;
    if pointer.kind() != "pointer_expression"
        || pointer.child_by_field_name("operator")?.kind() != "*"
    {
        return None;
    }
    let name = unparenthesized(pointer.child_by_field_name("argument")?)?;
    (name.kind() == "identifier").then_some(name)
}

fn unparenthesized(mut node: Syntax<'_>) -> Option<Syntax<'_>> {
    while node.kind() == "parenthesized_expression" {
        node = node.named_child(0)?;
    }
    Some(node)
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&CPP];
