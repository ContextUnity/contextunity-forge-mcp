use crate::acdd::support::MilestoneFixtureBuilder;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::{mcp_harness::McpTestHarness, Workspace};
use serde_json::{json, Value};

impl TaskHarness {
    /// Creates the physical task-list fixture without a Git runner baseline.
    pub fn task_list_workspace() -> Workspace {
        let workspace = Workspace::new();
        workspace.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: []\ntasks_db: tasks.sqlite\n",
        );
        workspace
    }

    pub fn mcp_workspace() -> Workspace {
        let workspace = Workspace::new();
        crate::acdd::support::initialize_runner_fixture(&workspace);
        workspace
    }

    pub fn mcp_dispatch_call(client: &mut McpTestHarness, tool: &str, mut args: Value) -> Value {
        args["format"] = json!("json");
        let response = client.call(tool, args).expect("MCP call should succeed").1;
        response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(response)
    }

    pub fn mcp_dispatch_fixture(
        name: &str,
        specs: &[(&str, &str)],
    ) -> (Workspace, McpTestHarness, Vec<String>) {
        let workspace = Self::mcp_workspace();
        workspace.write(
            "forge-mcp.yaml",
            "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
        );
        let mut manifest = MilestoneFixtureBuilder::new(&format!("m-{name}"))
            .title("Dispatch")
            .without_started_at()
            .invariants(&["isolated"]);
        for (reference, scope) in specs {
            manifest = manifest
                .task(reference, &[*scope])
                .task_scope_roots(&["src/"]);
            let file = if scope.ends_with('/') {
                format!("{scope}lib.rs")
            } else {
                (*scope).to_owned()
            };
            workspace.write(&file, "pub fn seam() {}\n");
        }
        workspace.write("docs/milestones/010-dispatch.md", &manifest.build());
        let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
        let synced = client
            .payload(
                "task_manage",
                json!({"action":"sync","milestone_ref":"docs/milestones/010-dispatch.md"}),
            )
            .expect("MCP payload should succeed");
        let ids = specs
            .iter()
            .map(|(reference, _)| {
                synced["tasks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|task| task["spec"]["task_ref"] == *reference)
                    .unwrap()["task_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        (workspace, client, ids)
    }

    pub fn mcp_dispatch_advance(
        workspace: &Workspace,
        client: &mut McpTestHarness,
        id: &str,
        stage: &str,
        worker: &str,
    ) -> anyhow::Result<Value> {
        let mut harness = Self::attach(workspace, id);
        harness.claim_gate_in_session(stage, worker, Some(&Self::session_for(worker)))?;
        let submitted = harness.submit_pass()?;
        let next = McpTestHarness::with_session(workspace, &Self::session_for(worker))?;
        std::mem::replace(client, next).finish()?;
        Ok(submitted)
    }
}
