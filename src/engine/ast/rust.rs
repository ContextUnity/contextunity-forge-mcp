pub fn language() -> tree_sitter::Language {
    tree_sitter_rust::language()
}
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_item" => Some("function"),
        "function_signature_item" => Some("function"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        "trait_item" => Some("trait"),
        "impl_item" => Some("impl"),
        "mod_item" => Some("module_declaration"),
        _ => None,
    }
}
