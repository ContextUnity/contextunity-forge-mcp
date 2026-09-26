fn main() {
    let proto_src = std::path::Path::new("vendor/tree-sitter-proto/src");
    let mut c_config = cc::Build::new();
    c_config.include(proto_src);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    c_config.file(proto_src.join("parser.c"));
    c_config.compile("tree-sitter-proto");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-proto/src/parser.c");
}
