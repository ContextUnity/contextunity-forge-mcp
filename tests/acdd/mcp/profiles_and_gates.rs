use serde_json::{json, Value};

use crate::acdd::support::MilestoneFixtureBuilder;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;

#[test]
fn task_claim_bundle_and_task_manage_context_returns_unified_agent_context() {
    let workspace = TaskHarness::mcp_workspace();

    workspace.write("AGENTS.md", "# Test guidance\n");
    let routing_adr = "---\ntitle: Task Routing Invariants\ndoc_type: adr\nstatus: accepted\n---\n# Task Routing Invariants\n- INV-ADR: Preserve [`route_task`](../../src/routing.rs#L2).\n- Decision: Admit every caller before changing the public signature.";
    workspace.write(
        "docs/reference/routing.md",
        "# Routing Reference\n\nSee [route_task](../../src/routing.rs#L2) for the `route_task` entry point.\n\n## Unrelated Rules\n- Invariant: Unrelated section rule.\n",
    );
    workspace.write(
        "docs/adr/001-task-routing.md",
        &routing_adr.replace('\n', "\r\n"),
    );
    workspace.write(
        "docs/adr/002-unrelated.md",
        "---\ntitle: Storage Layout\nstatus: accepted\n---\n# Storage Layout\nSQLite pages and retention.",
    );
    let mut rules = String::from("# Routing contract\n");
    for index in 0..6 {
        rules.push_str(&format!("- INV-C{index:02}: Preserve routing rule {index} through [`route_task`](../../src/routing.rs#L2).\n"));
    }
    workspace.write("docs/contracts/routing.md", &rules);
    for (path, source) in [
        (
            "src/routing.rs",
            "pub struct Router;\npub fn route_task() {}\n",
        ),
        ("src/unique.rs", "pub fn unique() {}\n"),
        ("src/minimal_routing.rs", "pub fn minimal_route_task() {}\n"),
        (
            "src/minimal_markdown.rs",
            "pub fn minimal_markdown_task() {}\n",
        ),
        ("tests/test_routing.rs", "#[test]\nfn test_routing() {}\n"),
        ("src/cli_routing.rs", "pub fn route_cli_task() {}\n"),
        (
            "tests/test_cli_routing.rs",
            "#[test]\nfn test_cli_routing() {}\n",
        ),
    ] {
        workspace.write(path, source);
    }
    let mut decision_rules = String::from("---\ntitle: Decision after rules\ndoc_type: adr\nstatus: accepted\n---\n# Decision after rules\n");
    for index in 0..6 {
        decision_rules.push_str(&format!("- INV-DEC{index:02}: Preserve decision rule {index} through [`unique`](../../src/unique.rs#L1).\n"));
    }
    decision_rules
        .push_str("Decision: Keep the complete decision after every bounded navigation rule.\n");
    workspace.write("docs/adr/003-decision-after-rules.md", &decision_rules);
    workspace.write("docs/adr/004-headed-decision.md", "---\ntitle: Headed Decision\ndoc_type: adr\nstatus: accepted\n---\n# Governance\n- INV-HEADED: Preserve the decision through [`unique`](../../src/unique.rs#L1).\n## Decision\nRetain the complete decision from its own section.\n");
    let task_specs: [(&str, &str, &[&str], &[&str]); 6] = [
        (
            "routing-task-with-a-long-identity-for-blocker-regression",
            "Deliver unified routing",
            &["src/routing.rs", "tests/test_routing.rs"],
            &[],
        ),
        (
            "cli-routing-task",
            "Deliver CLI unified routing",
            &["src/cli_routing.rs", "tests/test_cli_routing.rs"],
            &[],
        ),
        (
            "minimal-routing-task",
            "Deliver minimal routing",
            &["src/minimal_routing.rs"],
            &[],
        ),
        (
            "decision-context-task",
            "Preserve governing ADR decisions",
            &["src/unique.rs"],
            &[],
        ),
        (
            "dependency-context-task",
            "Preserve declared task dependencies",
            &["src/routing.rs"],
            &["routing-task-with-a-long-identity-for-blocker-regression"],
        ),
        (
            "minimal-markdown-context-task",
            "Render a minimal claim context",
            &["src/minimal_markdown.rs"],
            &[],
        ),
    ];
    let mut fixture = MilestoneFixtureBuilder::new("m-context")
        .title("Context Milestone")
        .without_status()
        .invariants(&[
            "routing-invariant",
            "authority-2",
            "authority-3",
            "authority-4",
            "authority-5",
            "authority-6",
            "authority-7",
        ]);
    for (task_ref, target, scope, dependencies) in task_specs {
        fixture = fixture
            .task_with_deps(task_ref, scope, dependencies)
            .task_target(target);
    }
    let manifest = fixture.build();
    workspace.write("docs/milestones/010-context.md", &manifest);
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    workspace.build_to(workspace.path(".forge/code-map.sqlite"));

    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let sync_res = client
        .payload(
            "task_manage",
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-context.md"}),
        )
        .expect("MCP payload should succeed");
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();
    let cli_task_id = sync_res["tasks"][1]["task_id"].as_str().unwrap();
    let sibling_task_id = cli_task_id;
    let minimal_task_id = sync_res["tasks"][2]["task_id"].as_str().unwrap();
    let decision_task_id = sync_res["tasks"][3]["task_id"].as_str().unwrap();
    let dependency_task_id = sync_res["tasks"][4]["task_id"].as_str().unwrap();
    let minimal_markdown_task_id = sync_res["tasks"][5]["task_id"].as_str().unwrap();

    // Post blackboard message
    let decision_post = client
        .payload(
            "task_blackboard",
            json!({
                "action": "post",
                "task_id": task_id,
                "topic": "decisions",
                "payload": "Route tasks through unified zero-shot context bundle",
                "author": "architect"
            }),
        )
        .expect("MCP payload should succeed");
    client
        .payload(
            "task_blackboard",
            json!({
                "action": "post",
                "task_id": sibling_task_id,
                "topic": "notes",
                "payload": "Sibling task context is milestone scoped",
                "author": "sibling-author"
            }),
        )
        .expect("MCP payload should succeed");

    // 1. MCP task_manage action: "context"
    let context_res = client
        .payload(
            "task_manage",
            json!({"action": "context", "task_id": task_id}),
        )
        .expect("MCP payload should succeed");
    let bundle = &context_res["context_bundle"];
    assert!(bundle.is_object(), "bundle must be present in response");

    // Check contract in bundle
    assert_eq!(bundle["contract"]["task_id"], task_id);
    assert_eq!(bundle["contract"]["target"], "Deliver unified routing");
    assert_eq!(bundle["contract"]["stage"], "contract");
    assert_eq!(bundle["contract"]["status"], "ready");
    assert_eq!(
        bundle["contract"]["allowed_scope"],
        json!(["src/routing.rs", "tests/test_routing.rs"])
    );
    assert_eq!(
        bundle["contract"]["invariants"],
        json!([
            "routing-invariant",
            "authority-2",
            "authority-3",
            "authority-4",
            "authority-5",
            "authority-6",
            "authority-7"
        ])
    );

    // Check guidance in bundle
    assert_eq!(bundle["guidance"]["stage"], "contract");
    assert_eq!(bundle["guidance"]["role"], "contract_author");
    let recommended_tools = bundle["guidance"]["recommended_tools"].as_array().unwrap();
    for tool in ["forge_read", "search_docs", "get_doc"] {
        assert!(recommended_tools.iter().any(|value| value == tool));
    }
    assert!(bundle["guidance"]["actionable_steps"]
        .as_array()
        .is_some_and(|steps| !steps.is_empty()));
    assert_eq!(
        bundle["guidance"]["subtask_dod"].as_array().unwrap().len(),
        5
    );

    // Check scope-to-ADR mapping
    let adrs = bundle["adrs"].as_array().unwrap();
    assert!(!adrs.is_empty(), "expected at least 1 mapped ADR");
    let adr = &adrs[0];
    assert_eq!(adr["path"], "docs/adr/001-task-routing.md");
    assert_eq!(adr["title"], "Task Routing Invariants");
    assert_eq!(adr["status"], "accepted");
    assert_eq!(adr["relevance"], "direct");
    assert_eq!(
        adrs.len(),
        2,
        "unrelated ADRs must not appear as governing context"
    );
    let reference = adrs
        .iter()
        .find(|document| document["path"] == "docs/reference/routing.md")
        .expect("reference documentation must appear in scoped context");
    assert!(reference["invariants_summary"]
        .as_array()
        .unwrap()
        .contains(&json!("Section: Routing Reference")));
    assert!(adr["invariants_summary"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line
            .as_str()
            .unwrap()
            .contains("Decision: Admit every caller")));
    let governing = &bundle["governing_invariants"];
    let direct = governing["direct"]
        .as_array()
        .expect("verified governing rules in task context");
    assert_eq!(direct.len(), 5);
    assert_eq!(
        direct
            .iter()
            .map(|rule| rule["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["INV-ADR", "INV-C00", "INV-C01", "INV-C02", "INV-C03"]
    );
    assert_eq!(governing["omitted_direct"], 2);
    assert!(direct
        .iter()
        .any(|rule| rule["source"]["path"] == "docs/contracts/routing.md"));
    assert_eq!(governing["transitive_ids"], json!([]));
    assert_eq!(governing["omitted_transitive"], 0);

    let symbols = bundle["scope_symbols"].as_array().unwrap();
    let route_task = symbols
        .iter()
        .find(|symbol| symbol["name"] == "route_task")
        .expect("scoped symbols must include route_task");
    assert!(route_task["documented_by"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| section == "docs/reference/routing.md#Routing Reference"));

    // Check covering tests
    let tests = bundle["covering_tests"].as_array().unwrap();
    assert!(
        tests
            .iter()
            .any(|t| t["path"].as_str() == Some("tests/test_routing.rs")),
        "covering_tests must include tests/test_routing.rs"
    );

    // Check blackboard
    let bb = bundle["blackboard"].as_array().unwrap();
    assert_eq!(bb.len(), 2);
    assert_eq!(bb[0]["topic"], "decisions");
    assert_eq!(bb[0]["author"], "architect");
    assert_eq!(
        bb[0]["payload"],
        "Route tasks through unified zero-shot context bundle"
    );
    assert!(bb
        .iter()
        .any(|message| { message["payload"] == "Sibling task context is milestone scoped" }));

    // 2. MCP task_claim returns the full bundle when the optional flag is omitted
    let claim_res = client
        .payload(
            "task_claim",
            json!({
                "task_id": task_id,
                "stage": "contract",
                "worker_id": "author-1",
                "worktree": workspace.root()
            }),
        )
        .expect("MCP payload should succeed");
    assert!(claim_res["context_bundle"].is_object());
    assert_eq!(
        claim_res["context_bundle"]["governing_invariants"],
        *governing
    );
    assert_eq!(
        claim_res["context_bundle"]["contract"]["invariants"],
        bundle["contract"]["invariants"]
    );
    assert!(claim_res["workflow_guidance"]["expected_evidence_template"].is_object());
    assert_eq!(
        claim_res["context_bundle"]["contract"]["worker_id"],
        "author-1"
    );
    assert_eq!(
        claim_res["context_bundle"]["adrs"][0]["relevance"],
        "direct"
    );
    assert_eq!(
        claim_res["context_bundle"]["blackboard"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // 3. CLI task claim returns the same bundle by default without --bundle
    let cli_claim = crate::common::cli::run_cli_json(
        &workspace,
        &[
            "task",
            "claim",
            cli_task_id,
            "--stage",
            "contract",
            "--worker",
            "cli-author",
            "--worktree",
            workspace.root().to_str().unwrap(),
        ],
    );
    assert!(
        cli_claim.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_claim.stderr)
    );
    let cli_claim_json: Value = serde_json::from_slice(&cli_claim.stdout).unwrap();
    assert!(cli_claim_json["context_bundle"].is_object());
    assert_eq!(
        cli_claim_json["context_bundle"]["contract"]["task_id"],
        cli_task_id
    );
    assert_eq!(
        cli_claim_json["context_bundle"]["contract"]["allowed_scope"],
        json!(["src/cli_routing.rs", "tests/test_cli_routing.rs"])
    );
    assert_eq!(
        cli_claim_json["context_bundle"]["blackboard"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // 4. CLI task context command
    let cli_context = crate::common::cli::run_cli_json(&workspace, &["task", "context", task_id]);
    assert!(
        cli_context.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_context.stderr)
    );
    let cli_json: Value = serde_json::from_slice(&cli_context.stdout).unwrap();
    assert!(cli_json["context_bundle"].is_object());
    assert_eq!(
        cli_json["context_bundle"]["governing_invariants"],
        *governing
    );
    assert_eq!(
        cli_json["context_bundle"]["contract"]["invariants"],
        bundle["contract"]["invariants"]
    );
    assert_eq!(cli_json["context_bundle"]["contract"]["task_id"], task_id);
    assert_eq!(
        cli_json["context_bundle"]["contract"]["depends_on"],
        json!([])
    );

    // 5. Minimal claim (bundle: false) prunes gates, attempts, findings, receipt, and spec
    let minimal_claim = client
        .payload(
            "task_claim",
            json!({
                "task_id": minimal_task_id,
                "stage": "contract",
                "worker_id": "author-minimal",
                "worktree": workspace.root(),
                "bundle": false
            }),
        )
        .expect("MCP payload should succeed");
    for field in [
        "context_bundle",
        "gates",
        "attempts",
        "findings",
        "receipt",
        "spec",
    ] {
        assert!(minimal_claim.get(field).is_none(), "{field}");
    }
    assert_eq!(minimal_claim["depends_on"], json!([]));
    assert!(minimal_claim["subtasks"].is_array());
    assert!(minimal_claim["workflow_guidance"].is_object());
    assert!(minimal_claim["allowed_write_scope"].is_array());
    assert_eq!(minimal_claim["task_id"], minimal_task_id);

    // 6. Milestone-scoped blackboard message (task_id IS NULL) with multi-byte Ukrainian text > 500 chars
    let ukr_payload = "Тестовий запис архітектури українською мовою для перевірки безпечного обрізання символів UTF-8. ".repeat(10);
    client
        .payload(
            "task_blackboard",
            json!({
                "action": "post",
                "milestone_ref": "docs/milestones/010-context.md",
                "topic": "notes",
                "payload": ukr_payload,
                "author": "ukr-architect"
            }),
        )
        .expect("MCP payload should succeed");
    let context_after = client
        .payload(
            "task_manage",
            json!({"action": "context", "task_id": task_id}),
        )
        .expect("MCP payload should succeed");
    let bb_after = context_after["context_bundle"]["blackboard"]
        .as_array()
        .unwrap();
    assert_eq!(
        bb_after.len(),
        3,
        "milestone-scoped message with task_id IS NULL must be included"
    );
    let ukr_msg = bb_after
        .iter()
        .find(|m| m["author"] == "ukr-architect")
        .unwrap();
    assert_eq!(ukr_msg["scope"], "milestone");
    assert!(ukr_msg["task_id"].is_null());
    assert!(ukr_msg["subtask_ref"].is_null());
    let ukr_text = ukr_msg["payload"].as_str().unwrap();
    assert!(ukr_text.ends_with("... [truncated]"));
    assert!(ukr_text.chars().count() <= 520);

    let task_msg = bb_after
        .iter()
        .find(|m| m["author"] == "architect")
        .unwrap();
    assert_eq!(task_msg["scope"], "task");
    assert_eq!(task_msg["task_id"], task_id);

    let sibling_msg = bb_after
        .iter()
        .find(|m| m["author"] == "sibling-author")
        .unwrap();
    assert_eq!(sibling_msg["scope"], "sibling");
    assert_eq!(sibling_msg["task_id"], sibling_task_id);

    let decision_context = client
        .payload(
            "task_manage",
            json!({"action": "context", "task_id": decision_task_id}),
        )
        .expect("MCP payload should succeed");
    let decision_adrs = decision_context["context_bundle"]["adrs"]
        .as_array()
        .unwrap();
    let mut decision_regressions = Vec::new();
    for (path, marker) in [
        (
            "docs/adr/003-decision-after-rules.md",
            "complete decision after every bounded navigation rule",
        ),
        (
            "docs/adr/004-headed-decision.md",
            "complete decision from its own section",
        ),
    ] {
        match decision_adrs.iter().find(|adr| adr["path"] == path) {
            Some(adr)
                if adr["invariants_summary"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|line| line.as_str().unwrap().contains(marker)) => {}
            Some(adr) => decision_regressions.push(format!(
                "required Decision evidence was omitted for {path}: {adr}"
            )),
            None => decision_regressions.push(format!(
                "governing ADR {path} was omitted from task context: {decision_adrs:?}"
            )),
        }
    }
    let cli_decision =
        crate::common::cli::run_cli_json(&workspace, &["task", "context", decision_task_id]);
    if cli_decision.status.success() {
        let cli_decision_json: Value = serde_json::from_slice(&cli_decision.stdout).unwrap();
        for path in [
            "docs/adr/003-decision-after-rules.md",
            "docs/adr/004-headed-decision.md",
        ] {
            if !cli_decision_json["context_bundle"]["adrs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|adr| {
                    adr["path"] == path
                        && adr["invariants_summary"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|line| line.as_str().unwrap().contains("Decision"))
                })
            {
                decision_regressions.push(format!(
                    "CLI task context omitted required Decision evidence for {path}: {}",
                    cli_decision_json["context_bundle"]["adrs"]
                ));
            }
        }
    } else {
        decision_regressions.push(format!(
            "CLI decision context failed: {}",
            String::from_utf8_lossy(&cli_decision.stderr)
        ));
    }
    assert!(
        decision_regressions.is_empty(),
        "Task5 Decision retention regressions: {decision_regressions:#?}"
    );

    let blocker_payload = format!(
        "First blocker detail: keep the exact routing entry point.\nSecond blocker detail: wait for the contract owner.\n{}",
        "Additional blocker evidence remains available through inspect. ".repeat(12)
    );
    let blocker_post = client
        .payload(
            "task_blackboard",
            json!({
                "action":"post",
                "task_id":task_id,
                "topic":"blockers",
                "gate":"contract",
                "payload":blocker_payload.clone(),
                "author":"release-coordinator"
            }),
        )
        .expect("MCP payload should succeed");
    let blocker_id = blocker_post["id"].as_u64().unwrap();
    let blocker_full = client
        .payload(
            "task_blackboard",
            json!({"action":"inspect","message_id":blocker_id}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(blocker_full["message"]["payload"], blocker_payload);
    let embedded_context = client
        .payload("task_manage", json!({"action":"context","task_id":task_id}))
        .expect("MCP payload should succeed");
    let embedded_blocker = embedded_context["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["id"] == blocker_id)
        .unwrap();
    assert_eq!(embedded_blocker["scope"], "task");
    assert_eq!(
        embedded_blocker["task_id"],
        blocker_full["message"]["task_id"]
    );

    let (_, context_response) = client
        .call("task_manage", json!({"action":"context","task_id":task_id}))
        .expect("MCP call should succeed");
    let context_markdown = context_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    for expected in [
        "# Task context",
        "## Lifecycle state",
        "Stage guidance",
        "Deliver unified routing",
        "src/routing.rs",
        "seam-test-first",
        "Expected evidence template",
        "NEVER_EDIT_RECEIPTS:",
        "## Blackboard messages",
        "### Blackboard decisions message",
        "BLOCKER (#",
        "by release-coordinator",
        "Scope:",
        "Gate:",
        "First blocker detail: keep the exact routing entry point.",
        "Second blocker detail: wait for the contract owner.",
        "... [truncated]",
        "Full body recovery:",
        &format!("message_id: {blocker_id}"),
    ] {
        assert!(
            context_markdown.contains(expected),
            "missing {expected:?} in:\n{context_markdown}"
        );
    }
    assert!(context_markdown.contains(&format!("BLOCKER (#{blocker_id}) by release-coordinator")));
    let blocker_callout = context_markdown
        .split(&format!(
            "> [!CAUTION] BLOCKER (#{blocker_id}) by release-coordinator"
        ))
        .nth(1)
        .and_then(|section| section.split("\n\n").next())
        .unwrap();
    assert!(
        blocker_callout.contains(&format!(
            "> Scope: `{}`",
            embedded_blocker["scope"].as_str().unwrap()
        )),
        "embedded blocker scope was not preserved: {blocker_callout}"
    );
    assert!(
        blocker_callout.contains(&format!(
            "> Task id: `{}`",
            embedded_blocker["task_id"].as_str().unwrap()
        )),
        "embedded blocker task identity was not preserved: {blocker_callout}"
    );
    assert!(context_markdown.contains("**scope**: `sibling`"));
    assert!(context_markdown.contains(&format!("**Task ID**: `{sibling_task_id}`")));
    assert!(context_markdown.contains(&format!("Task id: `{task_id}`")));
    assert!(context_markdown.contains("Gate: `contract`"));
    assert!(context_markdown.contains(task_id));
    assert!(context_markdown.contains("contract"));
    assert!(!context_markdown.contains(&format!("BLOCKER (#{})", decision_post["id"])));

    let cli_context_md = crate::common::cli::run_cli(
        &workspace,
        &["--format", "markdown", "task", "context", task_id],
    );
    assert!(
        cli_context_md.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_context_md.stderr)
    );
    let cli_markdown = String::from_utf8(cli_context_md.stdout).unwrap();
    assert!(cli_markdown.starts_with("# Task context"));
    for expected in [
        "## Lifecycle state",
        "Stage guidance",
        "Deliver unified routing",
        "src/routing.rs",
        "seam-test-first",
        "Expected evidence template",
        "NEVER_EDIT_RECEIPTS:",
        "## Blackboard messages",
        "### Blackboard decisions message",
        "BLOCKER (#",
        "by release-coordinator",
        "Scope:",
        "Gate:",
        "First blocker detail: keep the exact routing entry point.",
        "Second blocker detail: wait for the contract owner.",
        "... [truncated]",
        "Full body recovery:",
        &format!("message_id: {blocker_id}"),
    ] {
        assert!(
            cli_markdown.contains(expected),
            "missing {expected:?} in CLI markdown context:\n{cli_markdown}"
        );
    }
    assert!(cli_markdown.contains(&format!("BLOCKER (#{blocker_id}) by release-coordinator")));
    assert!(cli_markdown.contains(&format!("**Task ID**: `{cli_task_id}`")));

    let list_args = json!({
        "status":"all",
        "milestone_ref":"docs/milestones/010-context.md"
    });
    let listed = client
        .payload("task_list", list_args.clone())
        .expect("MCP payload should succeed");
    let dependency = listed["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["task_id"] == dependency_task_id)
        .unwrap();
    assert_eq!(
        dependency["depends_on"],
        json!(["routing-task-with-a-long-identity-for-blocker-regression"])
    );
    let (_, list_response) = client
        .call("task_list", list_args)
        .expect("MCP call should succeed");
    let list_markdown = list_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(list_markdown.contains(dependency_task_id));
    assert!(list_markdown.contains("Dependencies"));
    assert!(list_markdown.contains("routing-task-with-a-long-identity-for-blocker-regression"));

    let (_, claim_response) = client
        .call(
            "task_claim",
            json!({
                "task_id":decision_task_id,
                "stage":"contract",
                "worker_id":"markdown-author",
                "worktree":workspace.root()
            }),
        )
        .expect("MCP call should succeed");
    assert_ne!(claim_response["result"]["isError"], true);
    let claim_markdown = claim_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    for expected in [
        "# Task claim",
        "Stage guidance",
        "Scope and proof context",
        "CAUTION] BLOCKER",
        "by release-coordinator",
        "contract",
        "Expected evidence template",
    ] {
        assert!(
            claim_markdown.contains(expected),
            "missing {expected:?} in:\n{claim_markdown}"
        );
    }

    let (_, minimal_claim_response) = client
        .call(
            "task_claim",
            json!({
                "task_id":minimal_markdown_task_id,
                "stage":"contract",
                "worker_id":"minimal-markdown-author",
                "worktree":workspace.root(),
                "bundle":false
            }),
        )
        .expect("MCP call should succeed");
    let minimal_claim_markdown = minimal_claim_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(minimal_claim_markdown.contains("# Task claim"));
    assert!(minimal_claim_markdown.contains("Stage guidance"));
    assert!(minimal_claim_markdown.contains("Write scope"));
    assert!(!minimal_claim_markdown.contains("## Scope and proof context"));

    client.finish().expect("MCP server should finish");
}
