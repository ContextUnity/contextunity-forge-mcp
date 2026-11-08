use crate::acdd::support::{acdd_workspace, Workspace, DEFAULT_SEAM};
use crate::acdd::tasks::harness::TaskHarness;

pub(crate) struct CommitHistory {
    pub(super) root: Workspace,
    pub(super) base: String,
    pub(super) intermediate: String,
    pub(super) delivery: String,
}

impl TaskHarness {
    pub fn commit_history_fixture() -> anyhow::Result<CommitHistory> {
        let root = acdd_workspace(DEFAULT_SEAM, "");
        let base = TaskHarness::commit_source(
            &root,
            "src/lib.rs",
            "pub fn base() {}\n",
            "Baseline commit",
        )?;
        let intermediate = TaskHarness::commit_source(
            &root,
            "src/lib.rs",
            "pub fn base() {}\npub fn c1() {}\n",
            "Intermediate commit C1",
        )?;
        let delivery = TaskHarness::commit_source(
            &root,
            "src/lib.rs",
            "pub fn base() {}\npub fn c1() {}\npub fn delivery() {}\n",
            "Delivery commit D",
        )?;
        Ok(CommitHistory {
            root,
            base,
            intermediate,
            delivery,
        })
    }
}

impl CommitHistory {
    pub(super) fn validate(&self, commit: &str, scope: &[String]) -> anyhow::Result<()> {
        contextunity_forge_mcp::core::tasks::validate_commit_for_task(
            self.root.root(),
            commit,
            scope,
            Some("docs/milestones/010-test.md"),
            Some(&self.delivery),
            Some(&self.base),
        )
    }
}
