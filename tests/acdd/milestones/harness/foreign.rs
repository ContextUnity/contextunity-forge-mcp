use super::*;

pub(crate) const ACTIVE: &str = "docs/milestones/010-close.md";
pub(crate) const ARCHIVE: &str = "docs/milestones/archive/010-close.md";
pub(crate) const FOREIGN_WAITING: &str = "other/m-close:waiting";
pub(crate) const FOREIGN_KEEPER: &str = "refs/forge/state/forge-mcp/other/m-foreign/transfer";

pub(crate) struct ForeignHandoffFixture {
    pub root: MilestoneHarness,
    pub base: String,
    pub commit: String,
    pub task_id: String,
    pub source: String,
    pub delivered: String,
    pub evidence_file: String,
    pub foreign: std::path::PathBuf,
    pub selected_target: String,
    pub context: Value,
    pub foreign_proof: String,
    pub submissions: Vec<SubmittedGate>,
    pub(crate) foreign_parent: Workspace,
}

impl ForeignHandoffFixture {
    pub(crate) fn new() -> Self {
        let root = MilestoneHarness::linear("m-close");
        let source = root.document(ACTIVE);
        let base = git_head(root.root());
        let task_id = root.task_id("finish");
        tasks::manage(
            root.root(),
            tasks::Manage {
                action: tasks::ManageAction::ExtendScope,
                task_id: Some(task_id.clone()),
                paths: Some(vec!["src/extra.rs".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        root.write("src/lib.rs", "pub fn finish() {}\n");
        root.write("src/extra.rs", "pub fn admitted_extension() {}\n");
        let submissions = root
            .complete_task(CompletionMode::TaskRef("finish"))
            .unwrap();
        let commit = git_head(root.root());
        let evidence_file =
            fixture_evidence_file(root.root(), "m-close", "cargo test --all-targets", 7, 0);
        let delivered = root.document(ACTIVE);
        let foreign_parent = Workspace::new();
        let foreign = foreign_parent.path("linked");
        root.add_detached_worktree(&foreign, &commit).unwrap();
        MilestoneHarness::git_at(
            &foreign,
            &["commit", "--allow-empty", "-m", "foreign historical target"],
        )
        .unwrap();
        foreign_parent.write(
            "linked/.forge/acdd/profile.yaml",
            &serde_yaml::to_string(&fixture_profile()).unwrap(),
        );
        let selected_target = MilestoneHarness::git_text_at(&foreign, &["rev-parse", "HEAD"]);
        let documents: std::collections::BTreeMap<String, String> = serde_json::from_str(
            include_str!("../../fixtures/milestones/foreign_handoff.json"),
        )
        .unwrap();
        let write = |path: &str| foreign_parent.write(format!("linked/{path}"), &documents[path]);
        write("docs/milestones/020-foreign.md");
        MilestoneHarness::git_at(&foreign, &["add", "docs/milestones/020-foreign.md"]).unwrap();
        MilestoneHarness::git_at(&foreign, &["commit", "-m", "foreign active handoff"]).unwrap();
        let sync = |path: &str| {
            tasks::manage(
                &foreign,
                tasks::Manage {
                    action: tasks::ManageAction::Sync,
                    milestone_ref: Some(path.into()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        sync("docs/milestones/020-foreign.md");
        Self {
            root,
            base,
            commit,
            task_id,
            source,
            delivered,
            evidence_file,
            foreign,
            selected_target,
            context: Value::Null,
            foreign_proof: String::new(),
            submissions,
            foreign_parent,
        }
    }

    pub(crate) fn handoff(&self) -> anyhow::Result<Value> {
        milestone_engine::handoff(
            self.root.root(),
            "m-close",
            Some(&self.commit),
            &read_fixture_evidence(&self.evidence_file),
        )
    }
}
