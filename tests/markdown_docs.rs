use contextunity_forge_mcp::engine::docs;

#[test]
fn test_markdown_doc_extractor() {
    let source = r#"# Architecture Overview

General system architecture description.

## Invariants
1. Services MUST NEVER import internal database directly.
2. ContextRouter routes via verified JWT tokens.

## References
Consult `contextunity.core.tokens` and `ServiceClient`.
"#;
    let sections = docs::extract("docs/architecture.md", source, 1000.0).expect("markdown extract failed");
    assert!(!sections.is_empty());
    assert_eq!(sections[0].path, "docs/architecture.md");
    assert!(sections.iter().any(|s| s.section_title.contains("Architecture") || s.section_title.contains("Invariants")));
}
