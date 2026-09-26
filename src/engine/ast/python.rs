pub fn language() -> tree_sitter::Language {
    tree_sitter_python::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" => Some("function"),
        "class_definition" => Some("class"),
        _ => None,
    }
}
