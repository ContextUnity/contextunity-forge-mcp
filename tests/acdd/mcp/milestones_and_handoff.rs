use serde_json::{json, Value};

use contextunity_forge_mcp::core::format::{self, OutputFormat};
use contextunity_forge_mcp::core::tasks::profile::ProofKind;

use crate::acdd::support::{restart_mcp_session, review_proof, MilestoneFixtureBuilder};
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;

use super::in_process_cli;

macro_rules! mcp_payload {
    ($client:expr, $tool:expr, $args:expr $(,)?) => {
        $client.payload($tool, $args).expect("MCP payload should succeed")
    };
}

macro_rules! mcp_call {
    ($client:expr, $tool:expr, $args:expr $(,)?) => {
        $client.call($tool, $args).expect("MCP call should succeed")
    };
}

#[test]
fn linked_task_mcp_and_cli_resolve_repository_roots_and_guidance() {
    let main = TaskHarness::mcp_workspace();
    let linked = TaskHarness::mcp_workspace();
    let target = format!("Deliver {}", "large-record-marker ".repeat(300));
    let manifest = MilestoneFixtureBuilder::new("m-linked")
        .title("Linked")
        .without_started_at()
        .invariants(&["local-rules"])
        .task("first", &["src/", "src/lib.rs", "src/space # λ.rs", "src/missing.rs"])
        .task_target(&target)
        .build();
    main.write("docs/milestones/010-main.md", &manifest);
    main.write("AGENTS.md", "# Main rules\n");
    linked.write("src/lib.rs", "pub fn library() {}\n");
    linked.write("src/space # λ.rs", "pub fn scoped_file() {}\n");
    linked.write("AGENTS.md", "# Library rules\n");
    linked.write(
        "docs/milestones/010-linked.md",
        &manifest.replace('\n', "\r\n"),
    );
    main.write("forge-mcp.yaml", &format!("roots: []\ndocs: []\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n", linked.root().display()));
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\ntask_repository: traverse\ntask_project: traverse\ntasks_db: {}\n",
            main.root().join(".forge/tasks.sqlite").display()
        ),
    );
    let mut client = McpTestHarness::new(&main).expect("MCP new should succeed");
    let main_synced = mcp_payload!(client, "task_manage", json!({"action":"sync","milestone_ref":"docs/milestones/010-main.md"}));
    let main_id = main_synced["tasks"][0]["task_id"].as_str().unwrap().to_owned();
    let output = in_process_cli::run_cli_json(&main, &["task", "sync", "--workspace", "traverse"]);
    assert!( output.status.success(), "{}", String::from_utf8_lossy(&output.stderr) );
    let synced: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!( mcp_payload!(client, "task_list", json!({}))["tasks"] .as_array() .unwrap() .len(), 1 );
    let all = mcp_payload!(client, "task_list", json!({"repository":"all","milestone_status":"all"}));
    assert_eq!(all["tasks"].as_array().unwrap().len(), 2);
    let cli_all = in_process_cli::run_cli_json(
        &main,
        &["task", "list", "--repository", "all", "--milestone-status", "all"],
    );
    assert!(
        cli_all.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_all.stderr)
    );
    let cli_all: Value = serde_json::from_slice(&cli_all.stdout).unwrap();
    assert_eq!(cli_all, all);
    let first_workspace_page = mcp_payload!(client, "task_list", json!({"repository":"all","milestone_status":"all","limit":1,"offset":0}));
    assert_eq!(first_workspace_page["pagination"]["total"], 2);
    assert_eq!(first_workspace_page["pagination"]["returned"], 1);
    assert_eq!(first_workspace_page["pagination"]["next_offset"], 1);
    assert_eq!(first_workspace_page["pagination"]["has_more"], true);
    assert_eq!( first_workspace_page["workspaces"]["traverse/traverse"]["workspace_root"], linked.root().to_str().unwrap() );
    let cli_first_workspace_page = in_process_cli::run_cli_json(
        &main,
        &[
            "task",
            "list",
            "--repository",
            "all",
            "--milestone-status",
            "all",
            "--limit",
            "1",
            "--offset",
            "0",
        ],
    );
    assert!(
        cli_first_workspace_page.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_first_workspace_page.stderr)
    );
    let cli_first_workspace_page: Value =
        serde_json::from_slice(&cli_first_workspace_page.stdout).unwrap();
    assert_eq!(
        cli_first_workspace_page["pagination"],
        first_workspace_page["pagination"]
    );
    assert_eq!(
        cli_first_workspace_page["tasks"],
        first_workspace_page["tasks"]
    );
    let all_generation = first_workspace_page["pagination"]["generation"].as_str().unwrap().to_owned();
    let second_workspace_page = mcp_payload!(client, "task_list", json!({ "repository":"all", "milestone_status":"all", "limit":1, "offset":1, "generation":all_generation }));
    let mut paged_ids = vec![first_workspace_page["tasks"][0]["task_id"].as_str().unwrap().to_owned(), second_workspace_page["tasks"][0]["task_id"].as_str().unwrap().to_owned()];
    let mut expected_ids = vec![main_id, id.to_owned()];
    paged_ids.sort();
    expected_ids.sort();
    assert_eq!(paged_ids, expected_ids);
    let root_mixed_token = mcp_call!(client, "task_list", json!({ "repository":"traverse", "limit":1, "offset":1, "generation":all_generation }));
    assert!( root_mixed_token.1["error"].is_object() || root_mixed_token.1["result"]["isError"] == true );
    let worktree_mixed_token = mcp_call!(client, "task_list", json!({ "worktree":linked.root(), "repository":"all", "milestone_status":"all", "limit":1, "offset":1, "generation":all_generation }));
    assert!( worktree_mixed_token.1["error"].is_object() || worktree_mixed_token.1["result"]["isError"] == true );
    let selected = mcp_payload!(client, "task_list", json!({"repository":"traverse"}));
    assert_eq!(selected["workspace_root"], linked.root().to_str().unwrap());
    assert_eq!(selected["workspace"], "traverse");
    assert_eq!( selected["agents_guidance"], linked.root().join("AGENTS.md").to_str().unwrap() );
    assert_eq!(selected["tasks"][0]["task_id"], id);
    assert!(selected["tasks"][0]["target"] .as_str() .unwrap() .starts_with("Deliver "));
    assert_eq!(selected["tasks"][0]["status"], "ready");
    assert_eq!(selected["tasks"][0]["stage"], "contract");
    assert_eq!(selected["tasks"][0]["owner"], Value::Null);
    assert_eq!(selected["tasks"][0]["agent_type"], "worker");
    assert_eq!(selected["tasks"][0]["rev"], 1);
    assert_eq!(selected["tasks"][0]["depends_on"], json!([]));
    let inspected = mcp_payload!(client, "task_manage", json!({"action":"inspect","task_id":id}));
    assert!(inspected.get("digest").is_none());
    assert_eq!(inspected["workspace_root"], linked.root().to_str().unwrap());
    let (_, markdown_response) = mcp_call!(client, "task_manage", json!({"action":"inspect","task_id":id,"format":"markdown"}));
    crate::common::assertions::assert_rpc_succeeded(&markdown_response);
    assert_ne!( markdown_response["result"]["isError"], true, "{markdown_response}" );
    let inspect_markdown = markdown_response["result"]["content"][0]["text"].as_str().expect("task inspect should return Markdown text");
    assert!(inspect_markdown.contains("## Task record"));
    assert!(!inspect_markdown.contains("## Contract"));
    assert!(!inspect_markdown.contains("digest"));
    let raw_scope = json!(["src/", "src/lib.rs", "src/space # λ.rs", "src/missing.rs"]);
    assert_eq!(inspected["spec"]["scope"], raw_scope);
    assert_eq!(inspected["authority_snapshot"]["task_scope"], raw_scope);
    assert!(inspected["spec"]["scope"] .as_array() .unwrap() .iter() .all(Value::is_string));
    let expected_scope_uris = ["src", "src/lib.rs", "src/space%20%23%20%CE%BB.rs"]
        .map(|path| format!("file://{}/{path}", linked.root().display()));
    for uri in &expected_scope_uris {
        assert_eq!(inspect_markdown.matches(&format!("({uri})")).count(), 1);
    }
    if !inspect_markdown.contains("src/missing.rs") {
        panic!("MCP task inspect omitted the unresolved scope path: {inspect_markdown}");
    }
    let unresolved_uri = format!("(file://{}/src/missing.rs)", linked.root().display());
    assert!(!inspect_markdown.contains(&unresolved_uri));
    assert!(!inspect_markdown.contains("(link unavailable)"));

    let cli_inspection = in_process_cli::run_cli(
        &main,
        &["--format", "markdown", "task", "inspect", id],
    );
    assert!(
        cli_inspection.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_inspection.stderr)
    );
    let cli_markdown = String::from_utf8_lossy(&cli_inspection.stdout);
    for uri in &expected_scope_uris {
        assert_eq!(
            cli_markdown.matches(&format!("({uri})")).count(),
            1,
            "CLI task inspect omitted {uri}"
        );
    }
    assert!(
        cli_markdown.contains("src/missing.rs"),
        "CLI task inspect omitted the unresolved scope path"
    );
    assert!(
        !cli_markdown.contains(&unresolved_uri),
        "CLI task inspect linked a missing scope path"
    );
    assert!(
        !cli_markdown.contains("(link unavailable)"),
        "CLI task inspect rendered a link-unavailable marker"
    );

    let cli_json_inspection = in_process_cli::run_cli_json(
        &main,
        &["task", "inspect", id],
    );
    assert!(
        cli_json_inspection.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_json_inspection.stderr)
    );
    let cli_json_inspected: Value = serde_json::from_slice(&cli_json_inspection.stdout).unwrap();
    assert_eq!(cli_json_inspected["spec"]["scope"], raw_scope);

    let untrusted_findings = json!([
        {"path":"../../outside","uri":"file:///etc/passwd"},
        {"nested":{"path":"../nested-secret","uri":"http://attacker.invalid/nested"}},
        {
            "presentation_metadata":{"_presentation_scope_uris":{"../../inband-forgery":"file:///etc/passwd"}},
            "payload":{"path":"../../inband-forgery","uri":"file:///etc/passwd"}
        },
        {"body":"finding body retains ../../body-only and file:///etc/passwd as literal text"},
        [
            {"path":"../array-traversal","uri":"file:///etc/passwd"},
            {"path":"src/lib.rs","uri":"file:///etc/passwd"},
            {"path":"src/lib.rs","uri":"http://attacker.invalid/source"},
            {"path":"src/lib.rs","uri":"javascript:alert(1)"}
        ]
    ]);

    let pure_markdown = format::render(&inspected, OutputFormat::Markdown, "task_manage");
    assert!(pure_markdown.contains("src/missing.rs"));
    assert!(!pure_markdown.contains("file://"));

    let repeated_paths = [
        "src/tasks/lifecycle/presentation/equal_scope/recorded_task_scope_with_a_very_long_filename_that_makes_repetition_material.rs",
        "tests/acdd/fixtures/task_lifecycle/repeated_scope_authority_path_with_a_distinct_long_name.rs",
        "docs/reference/tasks/fixtures/inspection_scope_path_that_is_repeated_across_every_authority_projection.md",
        "src/engine/tasks/manifest/normalized_scope_path_with_another_deliberately_long_name.rs",
        "tests/acdd/contract/presentation/repeated_initial_scope_and_allowed_write_scope_case.rs",
    ];
    let repeated_target = "Render the inspected task contract once while preserving each distinct authority and lifecycle record ".repeat(4);
    let task_invariant = concat!(
        "TASK-EVIDENCE-PRESERVATION: Equal scope, target, proof policy, and task invariants ",
        "collapse in presentation. The contract digest hash is omitted. Milestone invariants, ",
        "revisions, receipts, gates, attempts, and findings stay visible."
    );
    let milestone_invariant = concat!(
        "INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading ",
        "and renders equal scope, target, proof policy, and task invariants once. A fixture ",
        "with those repeated fields is smaller in Markdown than in JSON. The contract digest ",
        "hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible."
    );
    let mut repeated = inspected.clone();
    repeated["spec"]["scope"] = json!(repeated_paths);
    repeated["authority_snapshot"]["task_scope"] = json!(repeated_paths);
    repeated["initial_scope"] = json!(repeated_paths);
    repeated["allowed_write_scope"] = json!(repeated_paths);
    let repeated_scope_uris = repeated_paths
        .iter()
        .map(|path| (path.to_string(), json!(format!("file:///{path}"))))
        .collect::<serde_json::Map<_, _>>();
    repeated["_presentation_scope_uris"] = Value::Object(repeated_scope_uris.clone());
    repeated["spec"]["target"] = json!(repeated_target);
    repeated["authority_snapshot"]["task_target"] = json!(repeated_target);
    repeated["goal"] = json!(repeated_target);
    repeated["spec"]["proof_policy"] = json!("seam-test-first");
    repeated["authority_snapshot"]["task_proof_policy"] = json!("seam-test-first");
    repeated["proof_policy"] = json!("seam-test-first");
    repeated["spec"]["invariants"] = json!([task_invariant]);
    repeated["authority_snapshot"]["task_invariants"] = json!([task_invariant]);
    repeated["authority_snapshot"]["milestone_invariants"] = json!([milestone_invariant]);
    repeated["applicable_invariants"] = json!([milestone_invariant, task_invariant]);
    repeated["claim_revision"] = json!(19);
    repeated["contract_revision"] = json!(7);
    repeated["receipt"] = json!({"commit":"retained-receipt-marker"});
    repeated["gates"] = json!([{"stage":"retained-gate-marker"}]);
    repeated["attempts"] = json!([{"worker_id":"retained-attempt-marker"}]);
    repeated["findings"] = json!([{"summary":"retained-finding-marker"}]);
    repeated["digest"] = json!("digest-must-not-be-rendered");
    let repeated_markdown = format::render(&repeated, OutputFormat::Markdown, "task_manage");
    for path in repeated_paths {
        assert_eq!( repeated_markdown .matches(&format!("[{path}](file:///{path})")) .count(), 1, "each equal scope path should render once as one link: {path}\n{repeated_markdown}" );
        assert_eq!( repeated_markdown.matches(path).count(), 2, "one semantic link contributes only its label and URI path: {path}\n{repeated_markdown}" );
    }
    assert_eq!(repeated_markdown.matches(&repeated_target).count(), 1);
    assert_eq!(repeated_markdown.matches("seam-test-first").count(), 1);
    assert_eq!(repeated_markdown.matches(task_invariant).count(), 1);
    assert_eq!(repeated_markdown.matches(milestone_invariant).count(), 1);
    assert!(!repeated_markdown.contains("digest-must-not-be-rendered"));
    assert!(repeated_markdown.contains("Claim revision") && repeated_markdown.contains("19"));
    assert!(repeated_markdown.contains("Contract revision") && repeated_markdown.contains("7"));
    for marker in [
        "retained-receipt-marker",
        "retained-gate-marker",
        "retained-attempt-marker",
        "retained-finding-marker",
    ] {
        assert!( repeated_markdown.contains(marker), "missing lifecycle evidence {marker}" );
    }
    assert!( repeated_markdown.len() < serde_json::to_vec(&repeated).unwrap().len(), "repeated-field Markdown must be smaller than JSON" );

    let extended_path =
        "src/tasks/lifecycle/presentation/added_scope_is_distinct_from_the_initial_scope.rs";
    let mut extended = repeated.clone();
    let mut extended_scope_uris = repeated_scope_uris;
    extended_scope_uris.insert(
        extended_path.to_owned(),
        json!(format!("file:///{extended_path}")),
    );
    extended["_presentation_scope_uris"] = Value::Object(extended_scope_uris);
    let initial_scope = json!(repeated_paths);
    let mut effective_paths = repeated_paths.to_vec();
    effective_paths.push(extended_path);
    let effective_scope = json!(effective_paths);
    extended["spec"]["scope"] = effective_scope.clone();
    extended["authority_snapshot"]["task_scope"] = initial_scope.clone();
    extended["initial_scope"] = initial_scope;
    extended["allowed_write_scope"] = effective_scope;
    let extended_markdown = format::render(&extended, OutputFormat::Markdown, "task_manage");
    assert!(extended_markdown.contains("Initial scope"));
    assert!(extended_markdown.contains("Extended scope"));
    for path in repeated_paths {
        assert_eq!( extended_markdown .matches(&format!("[{path}](file:///{path})")) .count(), 1, "initial scope path should remain visible once: {path}\n{extended_markdown}" );
        assert_eq!( extended_markdown.matches(path).count(), 2, "initial path should occur only in its link label and URI: {path}\n{extended_markdown}" );
    }
    assert_eq!( extended_markdown .matches(&format!("[{extended_path}](file:///{extended_path})")) .count(), 1, "added scope path remains distinct in the extended scope" );

    let mut distinct = repeated.clone();
    distinct["spec"]["target"] = json!("spec target marker");
    distinct["authority_snapshot"]["task_target"] = json!("authority target marker");
    distinct["goal"] = json!("response target marker");
    distinct["spec"]["proof_policy"] = json!("spec proof policy marker");
    distinct["authority_snapshot"]["task_proof_policy"] = json!("authority proof policy marker");
    distinct["proof_policy"] = json!("response proof policy marker");
    distinct["spec"]["invariants"] = json!(["spec task invariant marker"]);
    distinct["authority_snapshot"]["task_invariants"] = json!(["authority task invariant marker"]);
    distinct["applicable_invariants"] = json!(["response invariant marker"]);
    let distinct_markdown = format::render(&distinct, OutputFormat::Markdown, "task_manage");
    for marker in [
        "spec target marker",
        "authority target marker",
        "response target marker",
        "spec proof policy marker",
        "authority proof policy marker",
        "response proof policy marker",
        "spec task invariant marker",
        "authority task invariant marker",
        "response invariant marker",
    ] {
        assert!( distinct_markdown.contains(marker), "distinct authoritative values must remain visible: {marker}" );
    }

    assert_eq!( inspected["agents_guidance"], linked.root().join("AGENTS.md").to_str().unwrap() );
    client = restart_mcp_session(client, &main, &TaskHarness::session_for("builder"))
        .expect("MCP session restart should succeed");
    let claimed = mcp_payload!(client, "task_claim", json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":linked.root()}));
    let claim_markdown = format::render(&claimed, OutputFormat::Markdown, "task_claim");
    assert!(claim_markdown.contains("## Contract"));
    assert_eq!(claimed["agents_guidance"], inspected["agents_guidance"]);
    assert_eq!(claimed["applicable_invariants"], json!(["local-rules"]));
    let mut boundary_failures = Vec::new();
    let receipt_rule_present = claimed["workflow_guidance"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| {
            rule.as_str().is_some_and(|rule| {
                rule.starts_with("NEVER_EDIT_RECEIPTS:")
                    && rule.contains("receipt:")
                    && rule.contains("status:")
                    && rule.contains("docs/milestones/")
            })
        });
    if !receipt_rule_present {
        boundary_failures.push(
            "claim guidance must forbid hand editing milestone receipts and status".to_owned(),
        );
    }

    let posted = mcp_payload!(client, "task_blackboard", json!({"action":"post","scope":"task","task_id":id,"topic":"notes","payload":"canonical candidate coordination"}));
    let message_id = &posted["id"];
    let message = mcp_payload!(client, "task_blackboard", json!({"action":"inspect","message_id":message_id}));
    for extra in [
        json!({"scope":"task"}),
        json!({"task_id":id}),
        json!({"milestone_ref":"docs/milestones/010-linked.md"}),
        json!({"subtask_ref":"ignored-subtask"}),
        json!({"scope":"task","task_id":id,"milestone_ref":"docs/milestones/010-linked.md"}),
        json!({"scope":"task","task_id":id,"milestone_ref":"docs/milestones/010-linked.md","subtask_ref":"ignored-subtask"}),
    ] {
        let mut args = json!({"action":"inspect","message_id":message_id,"format":"json"});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (_, response) = mcp_call!(client, "task_blackboard", args.clone());
        if response["result"]["isError"] == true || response.get("error").is_some() {
            boundary_failures.push(format!(
                "inspect must tolerate redundant context selectors: {args}: {response}"
            ));
        } else {
            assert_eq!(crate::common::assertions::mcp_payload(&response), message);
        }
    }
    for scope in [
        json!({"scope":"task","task_id":id}),
        json!({"scope":"milestone","milestone_ref":"docs/milestones/010-main.md"}),
    ] {
        let mut read = scope.clone();
        read["action"] = json!("read");
        read["limit"] = json!(1);
        read["offset"] = json!(0);
        read["format"] = json!("json");
        let expected = mcp_payload!(client, "task_blackboard", read.clone());
        read["action"] = json!("inspect");
        let (_, response) = mcp_call!(client, "task_blackboard", read.clone());
        if response["result"]["isError"] == true || response.get("error").is_some() {
            boundary_failures.push(format!(
                "inspect without message_id must use scoped read: {read}: {response}"
            ));
        } else {
            assert_eq!(crate::common::assertions::mcp_payload(&response), expected);
        }
    }

    let (_, traversal) = mcp_call!(client, "task_manage", json!({"action":"extend_scope","task_id":id,"paths":["../foreign.rs"]}));
    assert_eq!(traversal["result"]["isError"], true);
    let synced = mcp_payload!(client, "task_manage", json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/milestones/010-linked.md"}));
    assert_eq!(synced["tasks"][0]["status"], "in_progress");

    let mut candidate_commit = None;
    let mut candidate_commits = Vec::new();
    let mut delivery_commit = None;
    let build_proof = json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let contract_proof =
        json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}});
    let delivery_proof = json!({"delivery_proof":{"status":"passed"}});
    let review_proof_data = review_proof("pass");
    let lifecycle = TaskHarness::attach(&main, id);
    let gates = lifecycle.executable_gates().unwrap();
    for gate_def in &gates {
        let stage = gate_def.id.as_str();
        let kind = gate_def.proof.kind();
        let is_contract = kind == Some(ProofKind::Contract);
        let is_command = kind == Some(ProofKind::Command);
        let is_review = kind == Some(ProofKind::Review);
        let is_delivery = kind == Some(ProofKind::Delivery);
        let worker = if is_contract {
            claimed["worker_id"]
                .as_str()
                .expect("pre-claimed contract worker")
                .to_owned()
        } else {
            lifecycle.worker_for_gate(gate_def)
        };
        client = restart_mcp_session(client, &main, &TaskHarness::session_for(&worker))
            .expect("MCP session restart should succeed");
        let mut claim = if is_contract {
            claimed.clone()
        } else {
            mcp_payload!(client, "task_claim", json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":linked.root()}))
        };
        let proof = if is_delivery {
            &delivery_proof
        } else if is_review {
            &review_proof_data
        } else if is_contract {
            &contract_proof
        } else {
            &build_proof
        };
        if is_contract || is_command {
            mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":id}));
        }
        let mut evidence =
            serde_json::to_value(TaskHarness::evidence_for_claim(&claim, proof.clone())).unwrap();
        if is_review {
            let candidate = TaskHarness::verified_revision_text(
                linked.root(),
                candidate_commit.as_deref().unwrap(),
            )
            .unwrap();
            if claim["context_bundle"]["candidate_snapshot"]["commit"] != candidate {
                boundary_failures.push(format!(
                    "candidate_snapshot.commit must preserve the full SHA: {}",
                    claim["context_bundle"]["candidate_snapshot"]
                ));
            }
            for length in [7, 12, 39, 40] {
                assert_eq!( contextunity_forge_mcp::core::tasks::canonical_commit_from_prefix( &candidate[..length], &candidate ) .as_deref(), Some(candidate.as_str()), "{length}-character SHA prefixes resolve to the canonical candidate" );
            }
            evidence["commit"] = json!(&candidate[..7]);

            let mut rejection_evidence = evidence.clone();
            rejection_evidence["proof"]["review_proof"]["decision"] = json!("reject");
            let rejection = mcp_payload!(client, "task_submit", json!({ "task_id":id, "stage":stage, "evidence":rejection_evidence, "action":"reject", "findings":untrusted_findings.clone() }));
            assert_eq!(rejection["status"], "ready");
            client = restart_mcp_session(client, &main, &TaskHarness::session_for("builder"))
                .expect("MCP session restart should succeed");
            let persisted_inspection = mcp_payload!(client, "task_manage", json!({"action":"inspect","task_id":id}));
            let serialized_findings = serde_json::to_string(&untrusted_findings).unwrap();
            assert!(persisted_inspection["findings"] .as_array() .unwrap() .iter() .any(|record| record["findings"] == serialized_findings));
            let persisted_context = mcp_payload!(client, "task_manage", json!({"action":"context","task_id":id}));
            assert_eq!( persisted_context["workflow_guidance"]["rejection"]["findings"], untrusted_findings, "{persisted_context}" );
            assert!(persisted_context.get("_presentation_scope_uris").is_none());
            let (_, findings_markdown_response) = mcp_call!(client, "task_manage", json!({"action":"context","task_id":id,"format":"markdown"}));
            crate::common::assertions::assert_rpc_succeeded(&findings_markdown_response);
            let findings_markdown = findings_markdown_response["result"]["content"][0]["text"]
                .as_str()
                .expect("persisted findings should remain visible in Markdown");
            for (label, uri) in [
                ("../../outside", "file:///etc/passwd"),
                ("../nested-secret", "http://attacker.invalid/nested"),
                ("../../inband-forgery", "file:///etc/passwd"),
                ("../array-traversal", "file:///etc/passwd"),
                ("src/lib.rs", "file:///etc/passwd"),
                ("src/lib.rs", "http://attacker.invalid/source"),
                ("src/lib.rs", "javascript:alert(1)"),
            ] {
                assert!( findings_markdown.contains(label), "missing literal finding {label}" );
                assert!( findings_markdown.contains(uri), "missing literal finding URI {uri}" );
                assert!( !findings_markdown.contains(&format!("[{label}]({uri})")), "untrusted finding became a clickable link: {label} -> {uri}\n{findings_markdown}" );
            }
            assert!(findings_markdown.contains("finding body retains ../../body-only"));

            let repair_claim = mcp_payload!(client, "task_claim", json!({"task_id":id,"stage":"build","worker_id":"builder","worktree":linked.root()}));
            mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":id}));
            let repair_result = mcp_payload!(client, "task_submit", json!({ "task_id":id, "stage":"build", "evidence":{ "task_id":id, "stage":"build", "claim_revision":repair_claim["claim_revision"], "contract_revision":1, "worker_id":"builder", "worktree":linked.root(), "proof":build_proof }, "action":"pass" }));
            let retry_snapshot = repair_result["snapshot"]["commit"]
                .as_str()
                .unwrap()
                .to_owned();
            candidate_commit = Some(retry_snapshot.clone());
            candidate_commits.push(retry_snapshot);
            client = restart_mcp_session(client, &main, &TaskHarness::session_for(&worker))
                .expect("MCP session restart should succeed");
            claim = mcp_payload!(client, "task_claim", json!({"task_id":id,"stage":"review","worker_id":worker,"worktree":linked.root()}));
            let candidate = candidate_commit.as_deref().unwrap();
            assert_eq!( claim["context_bundle"]["candidate_snapshot"]["commit"], candidate );
            evidence["claim_revision"] = claim["claim_revision"].clone();
            evidence["worker_id"] = claim["worker_id"].clone();
            evidence["commit"] = json!(&candidate[..7]);
        }
        let result = mcp_payload!(client, "task_submit", json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"}));
        let snapshot = result["snapshot"]["commit"].as_str().unwrap();
        if is_command {
            candidate_commit = Some(snapshot.to_owned());
            candidate_commits.push(snapshot.to_owned());
        }
        if is_delivery {
            delivery_commit = Some(snapshot.to_owned());
            assert_eq!(result["receipt"]["commit"], snapshot);
    let landed = TaskHarness::verified_revision_text(linked.root(), "HEAD").unwrap();
            if result["receipt"]["commit"] != landed {
                boundary_failures.push(format!(
                    "receipt.commit must preserve the full SHA: {}",
                    result["receipt"]["commit"]
                ));
            }
        } else {
            assert_eq!( result["snapshot"]["inspect_cmd"], contextunity_forge_mcp::core::tasks::snapshot_inspect_cmd(snapshot) );
        }
        assert_eq!( result["status"], if is_delivery { "completed" } else { "ready" } );
    }

    let candidate_commit = candidate_commit.unwrap();
    let delivery_commit = delivery_commit.unwrap();

    client
        .finish()
        .expect("MCP server should finish before restart");
    let mut client = McpTestHarness::new(&main).expect("MCP new should succeed");
    let restored = mcp_payload!(client, "task_manage", json!({"action":"inspect","task_id":id}));
    assert_eq!(restored["status"], "completed");
    assert_eq!(restored["receipt"]["commit"], delivery_commit);
    assert_eq!(restored["latest_snapshot"]["commit"], candidate_commit);
    assert_eq!( restored["latest_snapshot"]["inspect_cmd"], contextunity_forge_mcp::core::tasks::snapshot_inspect_cmd(&candidate_commit) );
    for gate in restored["gates"].as_array().unwrap() {
        let gate_commit = gate["commit"].as_str().unwrap();
        assert!( candidate_commits .iter() .any(|candidate| candidate == gate_commit) || gate_commit == delivery_commit );
        let evidence: Value = serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        assert_eq!(evidence["commit"], gate["commit"]);
    }
    assert_eq!(restored["agents_guidance"], inspected["agents_guidance"]);
    let landed =
        TaskHarness::verified_revision_text(linked.root(), "HEAD").unwrap();
    if restored["receipt"]["commit"] != landed {
        boundary_failures
            .push("persisted receipt API must preserve the full delivery SHA".to_owned());
    }
    assert_eq!( mcp_payload!(client, "task_list", json!({"repository":"traverse"}))["tasks"], json!([]) );
    main.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\nresponse:\n  max_output_bytes: 4096\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n",
            linked.root().display()
        ),
    );
    let mut bounded_client = McpTestHarness::new(&main).expect("MCP new should succeed");
    assert!( format::render(&restored, OutputFormat::Markdown, "task_manage").len() > 4096, "fixture task record must exceed the configured budget" );
    let (bounded_bytes, bounded_response) = mcp_call!(bounded_client, "task_manage", json!({"action":"inspect","task_id":id,"format":"markdown"}));
    assert!( bounded_bytes <= 4096, "configured 4-KiB task response was {bounded_bytes} bytes: {bounded_response}" );
    assert_eq!( bounded_response["result"]["isError"], true, "{bounded_response}" );
    assert!( bounded_response["result"]["content"][0]["text"] .as_str() .unwrap() .contains("Output exceeds the response byte limit"), "{bounded_response}" );
    assert!( boundary_failures.is_empty(), "{}", boundary_failures.join("\n") );
    client.finish().expect("MCP server should finish");
    bounded_client.finish().expect("MCP server should finish");
}

#[test]
fn task_context_candidate_snapshot_delivery_reject_and_completed_receipt() {
    let workspace = TaskHarness::mcp_workspace();
    let reviewer = TaskHarness::mcp_workspace();
    let manifest = MilestoneFixtureBuilder::new("m-snapshots")
        .title("Snapshots and Rejections")
        .without_started_at()
        .invariants(&["review-contour-check"])
        .task("candidate-task", &["src/"])
        .task_target("Deliver candidate")
        .task_proof_policy("direct-proof")
        .build();
    workspace.write("docs/milestones/010-snap.md", &manifest);
    reviewer.write("docs/milestones/010-snap.md", &manifest);
    workspace.write("forge-mcp.yaml", "roots: []\ndocs: []\ntasks_db: .forge/tasks.sqlite\n");
    reviewer.write("forge-mcp.yaml", &format!("roots: []\ndocs: []\ntasks_db: {}\n", workspace.root().join(".forge/tasks.sqlite").display()));

    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let sync_res = mcp_payload!(client, "task_manage", json!({"action": "sync", "milestone_ref": "docs/milestones/010-snap.md"}));
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    // 1. Pass contract proof without supplying a caller-selected commit.
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("author")).expect("MCP session restart should succeed");
    let contract_claim = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "contract", "worker_id": "author", "worktree": workspace.root()}));
    mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":task_id}));
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "contract", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&contract_claim, json!({"contract_proof": {"seam_test_ref": "tests/acdd/tasks.rs", "red_exit_code": 101}})) }));

    // 2. Pass build WITHOUT a commit in evidence
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("builder")).expect("MCP session restart should succeed");
    let build_claim = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.root()}));
    mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":task_id}));
    let first_build = mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "build", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&build_claim, json!({"command_proof": {"command": "rust-check-and-test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}})) }));

    let first_candidate = first_build["snapshot"]["commit"].as_str().unwrap();

    // The engine pins the scoped candidate even when build evidence omits a commit.
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("reviewer")).expect("MCP session restart should succeed");
    let review_claim = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.root()}));
    assert_eq!( review_claim["context_bundle"]["candidate_snapshot"]["commit"], first_candidate );

    // Reject review with findings -> task bounces back to build
    let review_reject_proof = review_proof("reject");
    let review_findings = json!({ "decision": "reject", "notes": "Need build commit", "blackboard": ["durable-review-evidence"], "blackboard_messages": ["durable-message-field-evidence"] });
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "review", "action": "reject", "evidence": TaskHarness::evidence_for_claim(&review_claim, json!(review_reject_proof)), "findings": review_findings }));

    // Build claim after review reject: verify unresolved_review_findings contains review_findings
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("builder")).expect("MCP session restart should succeed");
    let build_claim2 = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.root()}));
    assert_eq!( build_claim2["context_bundle"]["unresolved_review_findings"], review_findings );

    let build_context = mcp_payload!(client, "task_manage", json!({"action": "context", "task_id": task_id}));
    assert_eq!( build_context["context_bundle"]["unresolved_review_findings"], review_findings );
    let (_, response) = mcp_call!(client, "task_manage", json!({"action": "context", "task_id": task_id, "format": "markdown"}));
    let build_context_markdown = response["result"]["content"][0]["text"].as_str().expect("Build context should return Markdown text");
    assert!( build_context_markdown.contains("**blackboard**") && build_context_markdown.contains("durable-review-evidence") && build_context_markdown.contains("**blackboard_messages**") && build_context_markdown.contains("durable-message-field-evidence"), "Build context should render all review-finding fields: {build_context_markdown}" );

    mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":task_id}));
    let second_build = mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "build", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&build_claim2, json!({"command_proof": {"command": "rust-check-and-test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}})) }));

    let build_commit = second_build["snapshot"]["commit"].as_str().unwrap();

    // Rebuilding the unchanged scoped tree keeps the pinned candidate.
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("reviewer")).expect("MCP session restart should succeed");
    let review_claim2 = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.root()}));
    assert_eq!( review_claim2["context_bundle"]["candidate_snapshot"]["commit"], build_commit );

    // Pass review
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "review", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&review_claim2, review_proof("pass")) }));

    // 4. Claim deliver and REJECT delivery with findings
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("delivery-lead")).expect("MCP session restart should succeed");
    let deliver_claim = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "deliver", "worker_id": "delivery-lead", "worktree": workspace.root()}));
    // At deliver before completion, receipt must be None
    assert!(deliver_claim["context_bundle"].get("receipt").is_none());

    let delivery_findings = json!({"decision": "reject", "notes": "Missing changelog entry"});
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "deliver", "action": "reject", "evidence": TaskHarness::evidence_for_claim(&deliver_claim, json!({"delivery_proof": {"status": "rejected"}})), "findings": delivery_findings }));

    // Task bounced back to build: claim build and verify unresolved_review_findings contains delivery findings
    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("builder")).expect("MCP session restart should succeed");
    let rebounce_build_claim = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.root()}));
    assert_eq!( rebounce_build_claim["context_bundle"]["unresolved_review_findings"], delivery_findings );

    // 5. Complete build, review, deliver and verify receipt in context_bundle
    mcp_payload!(client, "task_manage", json!({"action":"verify","task_id":task_id}));
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "build", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&rebounce_build_claim, json!({"command_proof": {"command": "rust-check-and-test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}})) }));

    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("reviewer")).expect("MCP session restart should succeed");
    let review_claim3 = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.root()}));
    mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "review", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&review_claim3, review_proof("pass")) }));

    client = restart_mcp_session(client, &workspace, &TaskHarness::session_for("delivery-lead")).expect("MCP session restart should succeed");
    let deliver_claim2 = mcp_payload!(client, "task_claim", json!({"task_id": task_id, "stage": "deliver", "worker_id": "delivery-lead", "worktree": workspace.root()}));
    assert!(deliver_claim2["context_bundle"].get("receipt").is_none());

    let delivered = mcp_payload!(client, "task_submit", json!({ "task_id": task_id, "stage": "deliver", "action": "pass", "evidence": TaskHarness::evidence_for_claim(&deliver_claim2, json!({"delivery_proof": {"status": "passed"}})) }));
    assert_eq!( delivered["snapshot"]["commit"], delivered["receipt"]["commit"] );

    // Completed task: context_bundle contains receipt, root envelope does not
    let completed_context = mcp_payload!(client, "task_manage", json!({"action": "context", "task_id": task_id}));
    assert!( completed_context.get("receipt").is_none(), "root envelope must prune receipt" );
    assert!( completed_context["context_bundle"]["receipt"].is_object(), "context_bundle must contain receipt for completed task" );
    assert_eq!( completed_context["context_bundle"]["receipt"]["commit"], delivered["receipt"]["commit"] );
    assert_eq!( completed_context["context_bundle"]["latest_snapshot"]["commit"], build_commit );
    let inspected = mcp_payload!(client, "task_manage", json!({"action":"inspect","task_id":task_id}));
    let findings = inspected["findings"].as_array().unwrap();
    assert!(findings.len() >= 2);
    let review_finding = findings.iter().find(|record| record["findings"].as_str().is_some_and(|text| text.contains("durable-review-evidence"))).unwrap();
    let finding_text = review_finding["findings"].as_str().unwrap();
    assert_eq!( serde_json::from_str::<Value>(finding_text).unwrap(), review_findings );
    assert_eq!(inspected["latest_snapshot"]["commit"], build_commit);
    assert_eq!( inspected["receipt"]["commit"], delivered["receipt"]["commit"] );
    let (_, inspect_response) = mcp_call!(client, "task_manage", json!({"action":"inspect","task_id":task_id}));
    let inspect_markdown = inspect_response["result"]["content"][0]["text"].as_str().unwrap();
    for expected in [
        task_id,
        "Need build commit",
        "Missing changelog entry",
        "Latest snapshot",
        build_commit,
        "Durable receipt",
        delivered["receipt"]["commit"].as_str().unwrap(),
        finding_text,
    ] {
        assert!( inspect_markdown.contains(expected), "missing {expected:?} in task inspect Markdown:\n{inspect_markdown}" );
    }
    client.finish().expect("MCP server should finish");
}
