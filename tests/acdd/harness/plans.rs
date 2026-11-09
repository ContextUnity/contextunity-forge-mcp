use crate::acdd::support::{git_command, init_git, Workspace};
use contextunity_forge_mcp::{core::models::DocSection, engine::docs};

/// Canonical plan documents extracted through the production Markdown indexer.
/// Extraction stays in memory; execution fixtures explicitly materialize Git state.
pub(crate) struct PlanHarness {
    pub path: &'static str,
    pub sections: Vec<DocSection>,
    source: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) enum PlanFixture {
    UnknownDependency,
    Cycle,
    UnsafeScope,
    CancelledDependency,
    PendingSubtask,
    MissingSubtaskEvidence,
    CompletedSubtask,
}

impl PlanHarness {
    pub fn fixture(case: PlanFixture) -> Self {
        let source = match case {
            PlanFixture::PendingSubtask => include_str!("../fixtures/plans/pending-subtask.md"),
            PlanFixture::MissingSubtaskEvidence => {
                include_str!("../fixtures/plans/missing-subtask-evidence.md")
            }
            PlanFixture::CompletedSubtask => include_str!("../fixtures/plans/completed-subtask.md"),
            PlanFixture::UnknownDependency => {
                include_str!("../fixtures/plans/unknown-dependency.md")
            }
            PlanFixture::Cycle => include_str!("../fixtures/plans/cycle.md"),
            PlanFixture::UnsafeScope => include_str!("../fixtures/plans/unsafe-scope.md"),
            PlanFixture::CancelledDependency => {
                include_str!("../fixtures/plans/cancelled-dependency.md")
            }
        };
        Self::document("docs/plans/invalid.md", source)
    }
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

    pub fn materialize(&self, path: &str) -> Workspace {
        let root = Workspace::new();
        root.write(path, self.source);
        root.write(
            "forge-mcp.yaml",
            include_str!("../fixtures/plans/forge-mcp.yaml"),
        );
        root.write(
            "configs/custom.yaml",
            include_str!("../fixtures/plans/custom-config.yaml"),
        );
        root.write(
            "profiles/plan.yaml",
            include_str!("../fixtures/plans/snapshot.yaml"),
        );
        root.write(
            "profiles/rejected.yaml",
            include_str!("../fixtures/plans/rejected-profile.yaml"),
        );
        root.write(
            "profiles/commit.yaml",
            include_str!("../fixtures/plans/commit.yaml"),
        );
        root.write(
            "profiles/immutable-drift.yaml",
            include_str!("../fixtures/plans/immutable-drift.yaml"),
        );
        root.write("src/first.rs", "// initial plan scope\n");
        init_git(root.root());
        root
    }

    pub fn snapshots(&self, root: &Workspace) -> Vec<String> {
        let output = git_command(
            root.root(),
            &[
                "for-each-ref",
                "--format=%(objecttype) %(objectname) %(refname)",
                "refs/forge/",
            ],
        )
        .output()
        .expect("read plan snapshot refs");
        if !output.status.success() {
            panic!(
                "could not read plan snapshot refs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8(output.stdout)
            .expect("Git ref output is UTF-8")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn document(path: &'static str, source: &'static str) -> Self {
        let sections = docs::extract_with_details(path, source, 0.0)
            .expect("canonical plan Markdown must extract")
            .into_iter()
            .map(|(section, _)| section)
            .collect();
        Self {
            path,
            sections,
            source,
        }
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
