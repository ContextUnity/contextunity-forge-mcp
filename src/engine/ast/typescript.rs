pub fn language(path: &str, language: &str) -> tree_sitter::Language {
    if language == "javascript" {
        tree_sitter_javascript::language()
    } else if path.ends_with(".tsx") {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    }
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration"
        | "generator_function_declaration"
        | "arrow_function"
        | "function_expression" => Some("function"),
        "method_definition" | "method_signature" => Some("method"),
        "class_declaration" | "class" => Some("class"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}
