use super::*;

pub struct Yaml;
pub static YAML: Yaml = Yaml;
pub static PROFILES: &[&dyn LanguageProfile] = &[&YAML];

impl LanguageProfile for Yaml {
    fn id(&self) -> &'static str {
        "yaml"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["yaml", "yml"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        tree_sitter_yaml::language()
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        matches!(kind, "block_mapping_pair" | "flow_pair").then_some("variable")
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        node.child_by_field_name("key").map(|key| text(key, source))
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "variable"
    }
    fn pattern_wrapper(&self, kind: &str) -> bool {
        matches!(
            kind,
            "stream" | "document" | "block_node" | "block_mapping" | "flow_node"
        )
    }
    fn normalize_import(&self, _owner: &str, _module: &str) -> Option<ImportPath> {
        None
    }
    fn bindings(&self, _node: Syntax<'_>, _source: &str) -> ast::ScopeBindings {
        ast::ScopeBindings {
            all: Vec::new(),
            rebindings: Vec::new(),
        }
    }
    fn extract_file(&self, path: &str, source: &str, module: &str, facts: &mut Facts) -> Result<()> {
        parse_file(self, path, source, module, facts)
    }
}
