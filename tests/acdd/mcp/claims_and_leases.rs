use std::fs;
use std::process::Command;

use serde_json::{json, Value};

use crate::acdd::support::MilestoneFixtureBuilder;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_client::StdioClient;
use crate::common::mcp_harness::McpTestHarness;

use super::cli_payload;

#[test]
fn stdio_server_persists_the_environment_session_on_claim() {
    let harness = TaskHarness::disk();
    let gate = harness.executable_gates().unwrap().remove(0);
    let worker = harness.worker_for_gate(&gate);
    let session = "stdio-session-alpha";
    let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    command
        .arg("--root")
        .arg(harness.root())
        .arg("serve")
        .env("FORGE_AGENT_SESSION_ID", session)
        .env(
            "FORGE_EXTRACTION_CACHE_DIR",
            harness.root().join(".forge/extraction-cache"),
        );
    let mut client =
        StdioClient::with_command(&mut command).expect("stdio MCP client should start");
    let response = client
        .request_value(
            "tools/call",
            json!({
                "name":"task_claim",
                "arguments":{
                    "format":"json",
                    "task_id":harness.id,
                    "stage":gate.id,
                    "worker_id":worker,
                    "worktree":harness.root(),
                    "bundle":false
                }
            }),
        )
        .expect("stdio claim should succeed");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let claimed: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("stdio response should contain JSON text"),
    )
    .expect("stdio claim payload should be valid JSON");

    assert_eq!(
        harness.persisted_claim_session(claimed["claim_revision"].as_u64().unwrap()),
        session
    );
}

#[test]
fn mcp_task_claim_guidance_includes_rules_without_schema_bloat() {
    let workspace = TaskHarness::mcp_workspace();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/010-guidance.md",
        &MilestoneFixtureBuilder::new("m-guidance")
            .title("Claim guidance")
            .without_started_at()
            .task("full", &["src/"])
            .task_target("Check full claim guidance")
            .task("minimal", &["tests/"])
            .task_target("Check minimal claim guidance")
            .build(),
    );

    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let synced = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"docs/milestones/010-guidance.md"}),
        )
        .expect("MCP payload should succeed");
    let tasks = synced["tasks"].as_array().unwrap();
    let full_id = tasks[0]["task_id"].as_str().unwrap();
    let minimal_id = tasks[1]["task_id"].as_str().unwrap();
    let worktree = workspace.root().to_str().unwrap();
    let full_claim = client.payload(
        "task_claim",
        json!({"task_id":full_id,"stage":"contract","worker_id":"full-builder","worktree":worktree}),
    ).expect("MCP payload should succeed");
    let minimal_claim = client.payload(
        "task_claim",
        json!({"task_id":minimal_id,"stage":"contract","worker_id":"minimal-builder","worktree":worktree,"bundle":false}),
    ).expect("MCP payload should succeed");

    const PROTECTED_RULE: &str = "PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile are immutable while tasks are active. Declare task invariants inside task.invariants.";
    let expected_rules = [
        format!(
            "ISOLATION: Develop exclusively inside dedicated worktree '{}'.",
            workspace.root().display()
        ),
        "IMMUTABILITY: Keep admitted scope unchanged after review pass and during deliver."
            .to_owned(),
        "INDEPENDENCE: Review stage requires a worker_id distinct from the accepted builder."
            .to_owned(),
    ];
    let assert_guidance = |claim: &Value, surface: &str| {
        let guidance = &claim["workflow_guidance"];
        let guidance_text = serde_json::to_string(guidance).unwrap();
        let actual_rules = guidance["rules"].as_array().cloned().unwrap_or_default();
        let missing_rules: Vec<_> = expected_rules
            .iter()
            .filter(|expected| {
                !actual_rules
                    .iter()
                    .any(|actual| actual.as_str() == Some(expected.as_str()))
            })
            .collect();
        assert!(
            guidance_text.contains(PROTECTED_RULE) && missing_rules.is_empty(),
            "{surface} claim omitted protected guidance or operational rules; missing {missing_rules:?}; guidance: {guidance}"
        );
    };
    assert_guidance(&full_claim, "MCP full");
    assert_guidance(&minimal_claim, "MCP minimal");

    let (_, catalog) = client
        .request("tools/list", json!({}))
        .expect("MCP request should succeed");
    let catalog_text = serde_json::to_string(&catalog).unwrap();
    assert!(
        serde_json::to_vec(&catalog).unwrap().len()
            <= contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES,
        "MCP tool catalog exceeded the maximum response budget"
    );
    assert!(!catalog_text.contains(PROTECTED_RULE));
    for rule_prefix in ["ISOLATION:", "IMMUTABILITY:", "INDEPENDENCE:"] {
        assert!(
            !catalog_text.contains(rule_prefix),
            "runtime claim guidance must not be duplicated in tool schema metadata: {rule_prefix}"
        );
    }

    workspace.write("forge-mcp.yaml", "profile: minimal\n");
    let (_, minimal_catalog) = client
        .request("tools/list", json!({}))
        .expect("MCP request should succeed");
    let minimal_tools = minimal_catalog["result"]["tools"]
        .as_array()
        .expect("minimal profile should return its active tool catalog");
    assert_eq!(minimal_tools.len(), 2);
    assert_eq!(minimal_tools[0]["name"], "forge_guide");
    let minimal_guide = client
        .payload("forge_guide", json!({"topic":"query"}))
        .expect("MCP payload should succeed");
    assert_eq!(
        minimal_guide,
        json!({"topics":["query", "init", "adapter", "validate"]})
    );
    let mut preflight_failures = Vec::new();
    for topic in ["ast", "docs", "acdd"] {
        let guide = client
            .payload("forge_guide", json!({"topic":topic}))
            .expect("MCP payload should succeed");
        if guide != json!({}) {
            preflight_failures.push(format!(
                "minimal profile still advertises disabled {topic} capabilities: {guide}"
            ));
        }
    }

    workspace.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  acdd: true\n",
    );
    let acdd_without_blackboard = client
        .payload("forge_guide", json!({"topic":"acdd"}))
        .expect("MCP payload should succeed");
    if !acdd_without_blackboard["gates"].is_array() {
        preflight_failures.push(format!(
            "enabled ACDD guide lost its task guidance: {acdd_without_blackboard}"
        ));
    }
    for field in [
        "blackboard_topics",
        "blackboard_gate_targeting",
        "blackboard_schema_version",
    ] {
        if acdd_without_blackboard.get(field).is_some() {
            preflight_failures.push(format!(
                "disabled blackboard guidance leaked through forge_guide ({field}): {acdd_without_blackboard}"
            ));
        }
    }

    workspace.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  docs: true\n",
    );
    let docs_only_query = client
        .payload("forge_guide", json!({"topic":"query"}))
        .expect("MCP payload should succeed");
    if docs_only_query.get("recovery").is_some() {
        preflight_failures.push(format!(
            "docs-only query guide retained disabled codemap recovery advice: {docs_only_query}"
        ));
    }
    if docs_only_query.get("pages").is_none() || docs_only_query.get("documents").is_none() {
        preflight_failures.push(format!(
            "docs-only query guide lost enabled documentation guidance: {docs_only_query}"
        ));
    }

    let preflight = TaskHarness::mcp_workspace();
    preflight.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  acdd: true\nroots: []\ndocs: []\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let tool_profile = "version: 2\nmodules:\n  required: [codemap]\ngates:\n  - id: contract\n    role: contract_author\n    proof: contract\n  - id: build\n    role: builder\n    proof: command\n  - id: review\n    role: independent_reviewer\n    proof: review\n    reject_to: build\n    contours: guidance.contours\n  - id: deliver\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [review]\n";
    preflight.write(".forge/acdd/profile.yaml", tool_profile);
    let mut preflight_client = McpTestHarness::new(&preflight).expect("MCP new should succeed");
    let planned_tasks = [
        (
            "010-bundle-false.md",
            "m-bundle-false",
            "bundle_false",
            "src/preflight-bundle-false.rs",
        ),
        (
            "020-bundle-true.md",
            "m-bundle-true",
            "bundle_true",
            "src/preflight-bundle-true.rs",
        ),
        (
            "030-next.md",
            "m-next",
            "next_task",
            "src/preflight-next.rs",
        ),
    ];
    let mut task_ids = Vec::new();
    for (path, milestone_id, task_ref, scope) in planned_tasks {
        let manifest = MilestoneFixtureBuilder::new(milestone_id)
            .title("Planned preflight fixture")
            .status("planned")
            .task(task_ref, &[scope])
            .task_target("Check module requirements before claim")
            .build();
        let milestone_path = format!("docs/milestones/{path}");
        preflight.write(&milestone_path, &manifest);
        let synced = preflight_client
            .payload(
                "task_manage",
                json!({"action":"sync","milestone_ref":milestone_path}),
            )
            .expect("MCP payload should succeed");
        task_ids.push(
            synced["tasks"][0]["task_id"]
                .as_str()
                .expect("planned task should be indexed")
                .to_owned(),
        );
    }
    let before = task_ids
        .iter()
        .map(|task_id| {
            preflight_client
                .payload("task_manage", json!({"action":"inspect","task_id":task_id}))
                .expect("MCP payload should succeed")
        })
        .collect::<Vec<_>>();
    let worktree = preflight.root().to_str().unwrap();
    let claim_attempts = [
        (
            "task_manage",
            json!({"action":"next","worker_id":"next-builder","worktree":worktree}),
            "task_manage next",
        ),
        (
            "task_claim",
            json!({"task_id":task_ids[1],"stage":"contract","worker_id":"minimal-builder","worktree":worktree,"bundle":false}),
            "minimal task_claim",
        ),
        (
            "task_claim",
            json!({"task_id":task_ids[2],"stage":"contract","worker_id":"bundle-builder","worktree":worktree,"bundle":true}),
            "bundled task_claim",
        ),
    ];
    for (tool, arguments, label) in claim_attempts {
        let (_, response) = preflight_client
            .call(tool, arguments)
            .expect("MCP call should succeed");
        let is_error = response["result"]["isError"] == true || response.get("error").is_some();
        let response_text = serde_json::to_string(&response).unwrap_or_default();
        if !is_error || !response_text.contains("FEATURE_REQUIRED_BY_PROFILE") {
            preflight_failures.push(format!(
                "{label} did not fail with FEATURE_REQUIRED_BY_PROFILE: {response_text}"
            ));
        }
    }
    for (index, task_id) in task_ids.iter().enumerate() {
        let after = preflight_client
            .payload("task_manage", json!({"action":"inspect","task_id":task_id}))
            .expect("MCP payload should succeed");
        if after["status"] != "ready"
            || !after["worker_id"].is_null()
            || after["claim_revision"] != before[index]["claim_revision"]
        {
            preflight_failures.push(format!(
                "claim preflight changed planned task state: before={}, after={after}",
                before[index]
            ));
        }
    }
    for (path, _, _, _) in planned_tasks {
        let manifest =
            fs::read_to_string(preflight.root().join("docs/milestones").join(path)).unwrap();
        if !manifest.contains("\nstatus: planned\n") || manifest.contains("started_at:") {
            preflight_failures.push(format!(
                "claim preflight activated planned milestone {path}: {manifest}"
            ));
        }
    }

    let no_blackboard = TaskHarness::mcp_workspace();
    no_blackboard.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  acdd: true\n  codemap: true\n  docs: true\nroots: []\ndocs: []\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    no_blackboard.write(
        "docs/milestones/010-guidance.md",
        &MilestoneFixtureBuilder::new("m-no-blackboard")
            .title("No Blackboard")
            .without_started_at()
            .task("claim", &["src/"])
            .task_target("Check nonbundled module guidance")
            .build(),
    );
    let mut no_blackboard_client =
        McpTestHarness::new(&no_blackboard).expect("MCP new should succeed");
    let no_blackboard_sync = no_blackboard_client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"docs/milestones/010-guidance.md"}),
        )
        .expect("MCP payload should succeed");
    let no_blackboard_task = no_blackboard_sync["tasks"][0]["task_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let no_blackboard_worktree = no_blackboard.root().to_string_lossy();
    let unbundled_claim = no_blackboard_client.payload(
        "task_claim",
        json!({"task_id":no_blackboard_task,"stage":"contract","worker_id":"no-blackboard-builder","worktree":no_blackboard_worktree,"bundle":false}),
    ).expect("MCP payload should succeed");
    let inspected_claim = no_blackboard_client
        .payload(
            "task_manage",
            json!({"action":"inspect","task_id":no_blackboard_task}),
        )
        .expect("MCP payload should succeed");
    for (surface, response) in [
        ("task_claim bundle:false", &unbundled_claim),
        ("task_manage inspect", &inspected_claim),
    ] {
        let guidance = serde_json::to_string(&response["workflow_guidance"]).unwrap();
        if guidance.contains("task_blackboard") {
            preflight_failures.push(format!(
                "{surface} guidance mentioned disabled task_blackboard: {guidance}"
            ));
        }
    }

    let full_primary = TaskHarness::mcp_workspace();
    full_primary.write("forge-mcp.yaml", "profile: full\nroots: []\ndocs: []\n");
    let minimal_target = full_primary.root().join(".worktrees/selected-minimal");
    let full_target = full_primary.root().join(".worktrees/selected-full");
    fs::create_dir_all(&minimal_target).unwrap();
    fs::create_dir_all(&full_target).unwrap();
    fs::write(minimal_target.join("forge-mcp.yaml"), "profile: minimal\n").unwrap();
    fs::write(full_target.join("forge-mcp.yaml"), "profile: full\n").unwrap();
    let mut full_primary_client =
        McpTestHarness::new(&full_primary).expect("MCP new should succeed");
    let (_, selected_minimal) = full_primary_client
        .call(
            "task_list",
            json!({"worktree":minimal_target.to_string_lossy()}),
        )
        .expect("MCP call should succeed");
    let selected_minimal_text = serde_json::to_string(&selected_minimal).unwrap();
    if selected_minimal["result"]["isError"] != true
        || !selected_minimal_text.contains("FEATURE_DISABLED")
    {
        preflight_failures.push(format!(
            "selected minimal worktree accepted an ACDD task tool: {selected_minimal_text}"
        ));
    }

    full_primary.write("forge-mcp.yaml", "profile: minimal\n");
    let (_, selected_full) = full_primary_client
        .call(
            "task_list",
            json!({"worktree":full_target.to_string_lossy()}),
        )
        .expect("MCP call should succeed");
    let selected_full_text = serde_json::to_string(&selected_full).unwrap();
    if selected_full["result"]["isError"] == true || selected_full_text.contains("FEATURE_DISABLED")
    {
        preflight_failures.push(format!(
            "primary minimal profile blocked an ACDD tool enabled in selected full worktree: {selected_full_text}"
        ));
    }
    assert!(
        preflight_failures.is_empty(),
        "module-aware preflight and guidance must follow the selected workspace: {preflight_failures:#?}"
    );
    client.finish().expect("MCP server should finish");
    preflight_client.finish().expect("MCP server should finish");
    no_blackboard_client
        .finish()
        .expect("MCP server should finish");
    full_primary_client
        .finish()
        .expect("MCP server should finish");
}

#[test]
fn task_mcp_preserves_ready_claim_reset_selectors_and_diagnostic() {
    let workspace = TaskHarness::mcp_workspace();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.root().join("docs/milestones")).unwrap();
    workspace.write("docs/milestones/010-tasks.md","---\nid: m-pilot\ntitle: Pilot\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n");
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let synced = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md"}),
        )
        .expect("MCP payload should succeed");
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let ready = client
        .payload("task_list", json!({}))
        .expect("MCP payload should succeed");
    assert_eq!(ready["tasks"].as_array().unwrap().len(), 1);
    let cli = cli_payload(&workspace, &["task", "list"]).unwrap();
    assert_eq!(cli["tasks"].as_array().unwrap().len(), 1);
    let args = json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":workspace.root(),"format":"json"});
    let claimed = client
        .payload("task_claim", args.clone())
        .expect("MCP payload should succeed");
    let (_, collision) = client
        .call("task_claim", args)
        .expect("MCP call should succeed");
    assert_eq!(collision["result"]["isError"], true);
    let error: Value =
        serde_json::from_str(collision["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "TASK_ALREADY_CLAIMED");
    let collision_args =
        json!({"task_id":id,"stage":"contract","worker_id":"other","worktree":workspace.root()});
    let engine_error = contextunity_forge_mcp::engine::tasks::claim(
        workspace.root(),
        serde_json::from_value(collision_args.clone()).unwrap(),
    )
    .unwrap_err()
    .to_string();
    let (_, propagated) = client
        .call("task_claim", collision_args)
        .expect("MCP call should succeed");
    let propagated: Value =
        serde_json::from_str(propagated["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(propagated["error"]["message"], engine_error);
    assert!(
        engine_error.starts_with(&format!("TASK_ALREADY_CLAIMED: {id}. "))
            && engine_error.contains(". Do: ")
            && !engine_error.contains('\n'),
        "lifecycle diagnostics must use CODE: task_id. facts. Do: action on one line"
    );
    assert_eq!(
        client
            .payload("task_list", json!({}))
            .expect("MCP payload should succeed")["tasks"],
        json!([])
    );
    let active = client
        .payload("task_list", json!({"status":"in_progress"}))
        .expect("MCP payload should succeed");
    assert_eq!(active["tasks"][0]["task_id"], id);
    assert_eq!(active["tasks"][0]["owner"], "builder");
    assert_eq!(active["tasks"][0]["stage"], "contract");
    let reset = cli_payload(&workspace, &["task", "reset", id]).unwrap();
    assert!(
        reset["claim_revision"].as_u64().unwrap() > claimed["claim_revision"].as_u64().unwrap()
    );
    for args in [
        json!({"action":"inspect","task_id":id,"force":true}),
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md","paths":["src/new.rs"]}),
        json!({"unexpected":true}),
    ] {
        let name = if args.get("action").is_some() {
            "task_manage"
        } else {
            "task_list"
        };
        let (_, response) = client.call(name, args).expect("MCP call should succeed");
        assert!(response["result"]["isError"] == true || response.get("error").is_some());
    }
    client.finish().expect("MCP server should finish");
}
