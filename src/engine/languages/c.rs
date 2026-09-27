use super::*;
pub struct CProfile;
pub static C: CProfile = CProfile;
impl LanguageProfile for CProfile {
    fn id(&self) -> &'static str {
        "c"
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("c")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["c", "h"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_c::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        c_family::symbol_kind(kind)
    }
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        c_family::symbol(node)
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        c_family::symbol_name(node, source)
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "c"
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        c_family::include_path(self, owner, module)
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        c_family::bindings(self, node, source)
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
        if ctx.node.kind() == "call_expression" {
            call(ctx, facts, false);
        }
    }
}
pub static PROFILES: &[&dyn LanguageProfile] = &[&C];
