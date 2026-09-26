use tree_sitter::Language;

extern "C" {
    fn tree_sitter_proto() -> Language;
}

pub fn language() -> Language {
    unsafe { tree_sitter_proto() }
}

pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "message" => Some("class"),
        "service" => Some("service"),
        "rpc" => Some("function"),
        "enum" => Some("enum"),
        _ => None,
    }
}
