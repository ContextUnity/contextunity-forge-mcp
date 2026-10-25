use super::*;

pub(in crate::engine::languages) fn symbol_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" => Some("function"),
        "struct_specifier" | "union_specifier" => Some("struct"),
        "enum_specifier" => Some("enum"),
        "type_definition" => Some("type"),
        _ => None,
    }
}
pub(in crate::engine::languages) fn symbol(node: Syntax<'_>) -> Option<&'static str> {
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

pub(in crate::engine::languages) fn symbol_name<'a>(
    node: Syntax<'_>,
    source: &'a str,
) -> Option<&'a str> {
    node.child_by_field_name("name")
        .or_else(|| {
            node.child_by_field_name("declarator")
                .map(super::declarator)
        })
        .map(|n| text(n, source))
}
pub(in crate::engine::languages) fn bindings(
    profile: &dyn LanguageProfile,
    node: Syntax<'_>,
    source: &str,
) -> ast::ScopeBindings {
    super::bindings(
        profile,
        node,
        source,
        &["parameter_declaration", "optional_parameter_declaration"],
        &["init_declarator", "declaration", "field_declaration"],
    )
}
pub(in crate::engine::languages) fn include(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if ctx.node.kind() == "preproc_include" {
        if let Some(path) = field(ctx.node, ctx.source, "path") {
            super::import(ctx, facts, path, None);
        }
    }
}
pub(in crate::engine::languages) fn include_path(
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
