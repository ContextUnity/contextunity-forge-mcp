use contextunity_forge_mcp::{core::models::DocSection, engine::docs};

/// Canonical plan documents extracted through the production Markdown indexer.
/// Step parsing, DAG validation, and in-place execution await the AC-003 engine.
pub(crate) struct PlanHarness {
    pub path: &'static str,
    pub sections: Vec<DocSection>,
}

impl PlanHarness {
    pub fn linear() -> Self {
        Self::document(
            "docs/plans/linear.md",
            include_str!("../fixtures/plans/linear.md"),
        )
    }

    pub fn dag() -> Self {
        Self::document(
            "docs/plans/dag.md",
            include_str!("../fixtures/plans/dag.md"),
        )
    }

    fn document(path: &'static str, source: &str) -> Self {
        let sections = docs::extract_with_details(path, source, 0.0)
            .expect("canonical plan Markdown must extract")
            .into_iter()
            .map(|(section, _)| section)
            .collect();
        Self { path, sections }
    }
}

#[test]
fn canonical_plans_reach_document_indexing_with_step_text_intact() {
    for (harness, titles, dependency) in [
        (
            PlanHarness::linear(),
            vec![
                "Canonical Linear Plan",
                "1. Step 1: First Action",
                "2. Step 2: Second Action",
            ],
            "Step 1: First Action",
        ),
        (
            PlanHarness::dag(),
            vec![
                "Canonical DAG Plan",
                "1. Step A: Independent Core",
                "2. Step B: Extension Beta",
                "3. Step C: Extension Gamma",
            ],
            "Step A: Independent Core",
        ),
    ] {
        assert_eq!(
            harness
                .sections
                .iter()
                .map(|s| s.section_title.as_str())
                .collect::<Vec<_>>(),
            titles
        );
        for section in &harness.sections {
            assert_eq!(section.path, harness.path);
            assert_eq!(section.doc_type, "plan");
        }
        for step in &harness.sections[2..] {
            assert!(step
                .content
                .contains(&format!("**Depends On**: [\"{dependency}\"]")));
        }
    }
}
