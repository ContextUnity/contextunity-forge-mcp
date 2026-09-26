pub fn language() -> tree_sitter::Language {
    tree_sitter_go::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration" => Some("function"),
        "method_declaration" => Some("method"),
        "type_spec" => Some("type"),
        _ => None,
    }
}
