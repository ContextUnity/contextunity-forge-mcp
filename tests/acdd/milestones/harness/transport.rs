use super::*;

impl MilestoneHarness {
    pub(crate) fn handoff_context_at(
        root: &std::path::Path,
        selector: &str,
        target_base: &str,
    ) -> anyhow::Result<Value> {
        milestone_engine::handoff_context(root, selector, target_base)
    }

    pub(crate) fn init(&self, options: milestone_engine::Init) -> anyhow::Result<Value> {
        milestone_engine::init(self.root(), options)
    }

    pub(crate) fn handoff_context(
        &self,
        selector: &str,
        target_base: &str,
    ) -> anyhow::Result<Value> {
        Self::handoff_context_at(self.root(), selector, target_base)
    }

    pub(crate) fn show_result(&self, id: &str, full: bool) -> anyhow::Result<Value> {
        milestone_engine::show(self.root(), id, full)
    }

    pub(crate) fn db_recover(&self) -> anyhow::Result<Value> {
        milestone_engine::db_recover(self.root())
    }

    pub(crate) fn migrate(
        &self,
        action: &str,
        milestone_ref: Option<&str>,
    ) -> anyhow::Result<Value> {
        use contextunity_forge_mcp::cli::migrate::MigrateCommand;

        let milestone_ref = milestone_ref.map(str::to_owned);
        let command = match action {
            "preview" => MigrateCommand::Preview { milestone_ref },
            "apply" => MigrateCommand::Apply { milestone_ref },
            "verify" => MigrateCommand::Verify { milestone_ref },
            _ => anyhow::bail!("unknown migration action: {action}"),
        };
        contextunity_forge_mcp::cli::migrate::run(self.root(), command)
    }

    pub(crate) fn manage(
        &self,
        request: contextunity_forge_mcp::engine::tasks::Manage,
    ) -> anyhow::Result<Value> {
        contextunity_forge_mcp::engine::tasks::manage(self.root(), request)
    }

    pub(crate) fn claim_task(
        &self,
        task_id: &str,
        stage: &str,
        worker: &str,
    ) -> anyhow::Result<Value> {
        TaskHarness::claim_at(self.root(), task_id, stage, worker)
    }

    pub(crate) fn submit_request(
        &self,
        request: contextunity_forge_mcp::engine::tasks::Submit,
    ) -> anyhow::Result<Value> {
        contextunity_forge_mcp::engine::tasks::submit(self.root(), request)
    }

    pub(crate) fn execute_verification(
        &self,
        selector: &str,
        target_base: &str,
    ) -> anyhow::Result<Value> {
        milestone_engine::execute_milestone_verification(self.root(), selector, target_base)
    }

    pub(crate) fn workspace_names(&self) -> anyhow::Result<Vec<String>> {
        contextunity_forge_mcp::engine::tasks::list_workspaces(self.root())
    }
}
