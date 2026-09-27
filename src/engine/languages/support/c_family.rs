use super::*;

pub(super) fn symbol_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" => Some("function"),
        "struct_specifier" | "union_specifier" => Some("struct"),
        "enum_specifier" => Some("enum"),
        "type_definition" => Some("type"),
        _ => None,
    }
}
pub(super) fn symbol(node: Syntax<'_>) -> Option<&'static str> {
    if matches!(
        node.kind(),
        "struct_specifier" | "union_specifier" | "enum_specifier"
    ) && node.child_by_field_name("body").is_none()
        && !node.parent().is_some_and(|p| {
            p.kind() == "declaration" && p.child_by_field_name("declarator").is_none()
        })
    {
        return None;
    }
    symbol_kind(node.kind())
}

pub(super) fn symbol_name<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name("name")
        .or_else(|| {
            node.child_by_field_name("declarator")
                .map(syntax::declarator)
        })
        .map(|n| text(n, source))
}
pub(super) fn bindings(
    profile: &dyn LanguageProfile,
    node: Syntax<'_>,
    source: &str,
) -> ast::ScopeBindings {
    syntax::bindings(
        profile,
        node,
        source,
        &["parameter_declaration", "optional_parameter_declaration"],
        &["init_declarator", "declaration", "field_declaration"],
    )
}
pub(super) fn include(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if ctx.node.kind() == "preproc_include" {
        if let Some(path) = field(ctx.node, ctx.source, "path") {
            syntax::import(ctx, facts, path, None);
        }
    }
}
pub(super) fn include_path(
    profile: &dyn LanguageProfile,
    owner: &str,
    module: &str,
) -> Option<ImportPath> {
    let path = module.strip_prefix('"')?.strip_suffix('"')?;
    let normalized = relative_namespace(owner, path)?;
    Some(ImportPath {
        namespace: profile.module_name(&normalized),
        relative: true,
        symbol_path: false,
    })
}
