use crate::common::{mcp_client::StdioClient, Workspace as SharedWorkspace};
use contextunity_forge_mcp::core::format::{self, OutputFormat};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};

fn json_cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    command.args(["--format", "json"]);
    command
}

struct Workspace(PathBuf, SharedWorkspace);

impl std::ops::Deref for Workspace {
    type Target = SharedWorkspace;

    fn deref(&self) -> &Self::Target {
        &self.1
    }
}

impl Workspace {
    fn new() -> Self {
        let workspace = SharedWorkspace::new();
        super::support::init_git(workspace.root());
        Self(workspace.root().to_path_buf(), workspace)
    }

    fn write(&self, path: &str, content: &str) {
        self.1.write(path, content);
    }
}

type Client = StdioClient;

#[test]
fn linked_task_mcp_and_cli_resolve_repository_roots_and_guidance() {
    let main = Workspace::new();
    let linked = Workspace::new();
    fs::create_dir_all(main.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(linked.0.join("src")).unwrap();
    fs::create_dir_all(linked.0.join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-linked\ntitle: Linked\ndoc_type: contract\nstatus: active\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    main.write("docs/milestones/010-main.md", manifest);
    main.write("AGENTS.md", "# Main rules\n");
    linked.write("src/lib.rs", "pub fn library() {}\n");
    linked.write("AGENTS.md", "# Library rules\n");
    linked.write(
        "docs/milestones/010-linked.md",
        &manifest.replace('\n', "\r\n"),
    );
    main.write("forge-mcp.yaml", &format!("roots: []\ndocs: []\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n", linked.0.display()));
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\ntask_repository: traverse\ntask_project: traverse\ntasks_db: {}\n",
            main.0.join(".forge/tasks.sqlite").display()
        ),
    );
    let mut client = Client::new(&main);
    client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-main.md"}),
    );
    let output = json_cli()
        .args([
            "--root",
            main.0.to_str().unwrap(),
            "task",
            "sync",
            "--workspace",
            "traverse",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let synced: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!(
        client.payload("task_list", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let all = client.payload("task_list", json!({"repository":"all"}));
    assert_eq!(all["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(
        all["workspaces"]["traverse/traverse"]["workspace_root"],
        linked.0.to_str().unwrap()
    );
    let selected = client.payload("task_list", json!({"repository":"traverse"}));
    assert_eq!(selected["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(selected["workspace"], "traverse");
    assert_eq!(
        selected["agents_guidance"],
        linked.0.join("AGENTS.md").to_str().unwrap()
    );
    assert_eq!(
        selected["tasks"][0],
        json!({
            "task_id":id,"target":"Deliver","status":"ready","stage":"contract",
            "owner":null,"agent_type":"worker","rev":1,"depends_on":[]
        })
    );
    let output = json_cli()
        .args([
            "--root",
            main.0.to_str().unwrap(),
            "task",
            "list",
            "--repository",
            "all",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        all
    );
    let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    assert!(inspected.get("digest").is_none());
    assert_eq!(inspected["workspace_root"], linked.0.to_str().unwrap());
    let (_, markdown_response) = client.call(
        "task_manage",
        json!({"action":"inspect","task_id":id,"format":"markdown"}),
    );
    crate::common::assertions::assert_rpc_succeeded(&markdown_response);
    assert_ne!(
        markdown_response["result"]["isError"], true,
        "{markdown_response}"
    );
    let inspect_markdown = markdown_response["result"]["content"][0]["text"]
        .as_str()
        .expect("task inspect should return Markdown text");
    assert!(inspect_markdown.contains("## Task record"));
    assert!(!inspect_markdown.contains("## Contract"));
    assert!(!inspect_markdown.contains("digest"));
    assert_eq!(
        inspect_markdown,
        format::render(&inspected, OutputFormat::Markdown, "task_manage")
    );

    let repeated_paths = [
        "src/tasks/lifecycle/presentation/equal_scope/recorded_task_scope_with_a_very_long_filename_that_makes_repetition_material.rs",
        "tests/acdd/fixtures/task_lifecycle/repeated_scope_authority_path_with_a_distinct_long_name.rs",
        "docs/reference/tasks/fixtures/inspection_scope_path_that_is_repeated_across_every_authority_projection.md",
        "src/engine/tasks/manifest/normalized_scope_path_with_another_deliberately_long_name.rs",
        "tests/acdd/contract/presentation/repeated_initial_scope_and_allowed_write_scope_case.rs",
    ];
    let repeated_target = concat!(
        "Render the inspected task contract once while preserving each distinct ",
        "authority and lifecycle record "
    )
    .repeat(4);
    let task_invariant = concat!(
        "TASK-EVIDENCE-PRESERVATION: Equal scope, target, proof policy, and task invariants ",
        "collapse in presentation. The contract digest hash is omitted. Milestone ",
        "invariants, revisions, receipts, gates, attempts, and findings stay visible."
    );
    let milestone_invariant = concat!(
        "INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record ",
        "heading and renders equal scope, target, proof policy, and task invariants once. ",
        "A fixture with those repeated fields is smaller in Markdown than in JSON. The ",
        "contract digest hash is omitted from task responses. Revisions, receipts, gates, ",
        "attempts, and findings stay visible."
    );
    let mut repeated = inspected.clone();
    repeated["spec"]["scope"] = json!(repeated_paths);
    repeated["authority_snapshot"]["task_scope"] = json!(repeated_paths);
    repeated["initial_scope"] = json!(repeated_paths);
    repeated["allowed_write_scope"] = json!(repeated_paths);
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
        assert_eq!(
            repeated_markdown.matches(path).count(),
            1,
            "each equal scope path should render once: {path}\n{repeated_markdown}"
        );
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
        assert!(
            repeated_markdown.contains(marker),
            "missing lifecycle evidence {marker}"
        );
    }
    assert!(
        repeated_markdown.len() < serde_json::to_vec(&repeated).unwrap().len(),
        "repeated-field Markdown must be smaller than JSON"
    );

    let extended_path =
        "src/tasks/lifecycle/presentation/added_scope_is_distinct_from_the_initial_scope.rs";
    let mut extended = repeated.clone();
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
        assert_eq!(
            extended_markdown.matches(path).count(),
            1,
            "initial scope path should remain visible once: {path}\n{extended_markdown}"
        );
    }
    assert_eq!(extended_markdown.matches(extended_path).count(), 1);

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
        assert!(
            distinct_markdown.contains(marker),
            "distinct authoritative values must remain visible: {marker}"
        );
    }
    assert_eq!(
        inspected["agents_guidance"],
        linked.0.join("AGENTS.md").to_str().unwrap()
    );
    let claimed = client.payload(
        "task_claim",
        json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":linked.0}),
    );
    let claim_markdown = format::render(&claimed, OutputFormat::Markdown, "task_claim");
    assert!(claim_markdown.contains("## Contract"));
    assert_eq!(claimed["agents_guidance"], inspected["agents_guidance"]);
    assert_eq!(claimed["applicable_invariants"], json!(["local-rules"]));
    let mut boundary_failures = Vec::new();
    if !claimed["workflow_guidance"]["rules"]
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
        })
    {
        boundary_failures.push(
            "claim guidance must forbid hand editing milestone receipts and status".to_owned(),
        );
    }
    let posted = client.payload(
        "task_blackboard",
        json!({"action":"post","scope":"task","task_id":id,"topic":"notes","payload":"canonical candidate coordination"}),
    );
    let message_id = &posted["id"];
    let message = client.payload(
        "task_blackboard",
        json!({"action":"inspect","message_id":message_id}),
    );
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
        let (_, response) = client.call("task_blackboard", args.clone());
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
        let expected = client.payload("task_blackboard", read.clone());
        read["action"] = json!("inspect");
        let (_, response) = client.call("task_blackboard", read.clone());
        if response["result"]["isError"] == true || response.get("error").is_some() {
            boundary_failures.push(format!(
                "inspect without message_id must use scoped read: {read}: {response}"
            ));
        } else {
            assert_eq!(crate::common::assertions::mcp_payload(&response), expected);
        }
    }
    let (_, traversal) = client.call(
        "task_manage",
        json!({"action":"extend_scope","task_id":id,"paths":["../foreign.rs"]}),
    );
    assert_eq!(traversal["result"]["isError"], true);
    let synced = client.payload("task_manage", json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/milestones/010-linked.md"}));
    assert_eq!(synced["tasks"][0]["status"], "in_progress");
    let mut candidate_commit = None;
    let mut delivery_commit = None;
    let build_proof = json!({"command_proof":{"command":"cargo test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let contract_proof =
        json!({"contract_proof":{"seam_test_ref":"tests/acdd/mcp.rs","red_exit_code":101}});
    let delivery_proof = json!({"delivery_proof":{"status":"passed"}});
    let contours: serde_json::Map<String, Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"linked public-seam verification"}),
                )
            })
            .collect();
    let review_proof = json!({"review_proof":{"decision":"pass","contours":contours}});
    for (gate, stage) in contextunity_forge_mcp::core::tasks::GATES
        .iter()
        .enumerate()
    {
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let claim = if gate == 0 {
            claimed.clone()
        } else {
            client.payload(
                "task_claim",
                json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":linked.0}),
            )
        };
        let proof = if gate == 3 {
            &delivery_proof
        } else if gate == 2 {
            &review_proof
        } else if gate == 0 {
            &contract_proof
        } else {
            &build_proof
        };
        let mut evidence = json!({"task_id":id,"stage":stage,"claim_revision":claim["claim_revision"],"contract_revision":1,"worker_id":worker,"worktree":linked.0,"proof":proof});
        if gate == 2 {
            let candidate = contextunity_forge_mcp::core::git::git_stdout(
                &linked.0,
                &[
                    "rev-parse",
                    "--verify",
                    candidate_commit.as_deref().unwrap(),
                ],
                "TEST_GIT",
            )
            .unwrap();
            if claim["context_bundle"]["candidate_snapshot"]["commit"] != candidate {
                boundary_failures.push(format!(
                    "candidate_snapshot.commit must preserve the full SHA: {}",
                    claim["context_bundle"]["candidate_snapshot"]
                ));
            }
            for length in [7, 12, 39, 40] {
                assert_eq!(
                    contextunity_forge_mcp::core::tasks::canonical_commit_from_prefix(
                        &candidate[..length],
                        &candidate
                    )
                    .as_deref(),
                    Some(candidate.as_str()),
                    "{length}-character SHA prefixes resolve to the canonical candidate"
                );
            }
            evidence["commit"] = json!(&candidate[..7]);
        }
        let result = client.payload(
            "task_submit",
            json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"}),
        );
        let snapshot = result["snapshot"]["commit"].as_str().unwrap();
        if gate == 1 {
            candidate_commit = Some(snapshot.to_owned());
        }
        if gate == 3 {
            delivery_commit = Some(snapshot.to_owned());
            assert_eq!(result["receipt"]["commit"], snapshot);
            let landed = contextunity_forge_mcp::core::git::git_stdout(
                &linked.0,
                &["rev-parse", "--verify", "HEAD"],
                "TEST_GIT",
            )
            .unwrap();
            if result["receipt"]["commit"] != landed {
                boundary_failures.push(format!(
                    "receipt.commit must preserve the full SHA: {}",
                    result["receipt"]["commit"]
                ));
            }
        } else {
            assert_eq!(
                result["snapshot"]["inspect_cmd"],
                contextunity_forge_mcp::core::tasks::snapshot_inspect_cmd(snapshot)
            );
        }
        assert_eq!(
            result["status"],
            if gate == 3 { "completed" } else { "ready" }
        );
    }
    drop(client);
    let mut client = Client::new(&main);
    let restored = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    let candidate_commit = candidate_commit.unwrap();
    let delivery_commit = delivery_commit.unwrap();
    assert_eq!(restored["status"], "completed");
    assert_eq!(restored["receipt"]["commit"], delivery_commit);
    assert_eq!(restored["latest_snapshot"]["commit"], candidate_commit);
    assert_eq!(
        restored["latest_snapshot"]["inspect_cmd"],
        contextunity_forge_mcp::core::tasks::snapshot_inspect_cmd(&candidate_commit)
    );
    for gate in restored["gates"].as_array().unwrap() {
        let gate_commit = gate["commit"].as_str().unwrap();
        assert!(gate_commit == candidate_commit || gate_commit == delivery_commit);
        let evidence: Value = serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        assert_eq!(evidence["commit"], gate["commit"]);
    }
    assert_eq!(restored["agents_guidance"], inspected["agents_guidance"]);
    let landed = contextunity_forge_mcp::core::git::git_stdout(
        &linked.0,
        &["rev-parse", "--verify", "HEAD"],
        "TEST_GIT",
    )
    .unwrap();
    if restored["receipt"]["commit"] != landed {
        boundary_failures
            .push("persisted receipt API must preserve the full delivery SHA".to_owned());
    }
    assert_eq!(
        client.payload("task_list", json!({"repository":"traverse"}))["tasks"],
        json!([])
    );
    assert!(
        boundary_failures.is_empty(),
        "{}",
        boundary_failures.join("\n")
    );
}

#[test]
fn task_stdio_lifecycle_submits_inline_evidence_in_independent_worktrees() {
    use contextunity_forge_mcp::core::tasks::{gates::REVIEW_CONTOURS, GATES};
    let builder = Workspace::new();
    let reviewer = Workspace::new();
    let source = r#"---
id: m-handoff
title: Handoff
doc_type: contract
status: active
deferred_defects:
  - id: DEFECT-E2E-001
    source: review_findings
    title: Typed defect survives the MCP task lifecycle
    path: src/engine/tasks.rs
    disposition: subsequent_milestone
    notes: Preserved through receipt rewriting
---
```yaml
task_ref: delivery
target: Deliver
proof_policy: seam-test-first
scope: [src/]
```
"#;
    for workspace in [&builder, &reviewer] {
        fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
        workspace.write(
            "docs/milestones/010-tasks.md",
            &source.replace('\n', "\r\n"),
        );
    }
    builder.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntask_repository: contextunity\ntask_project: tooling\n",
    );
    reviewer.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\ntask_repository: contextunity\ntask_project: tooling\ntasks_db: {}\n",
            builder.0.join(".forge/tasks.sqlite").display()
        ),
    );
    let mut client = Client::new(&builder);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md"}),
    );
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!(id, "tooling/m-handoff:delivery");
    for action in ["preview", "apply", "verify"] {
        let output = json_cli()
            .args([
                "--root",
                builder.0.to_str().unwrap(),
                "migrate",
                action,
                "docs/milestones/010-tasks.md",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut candidate_commit = None;
    let mut delivery_commit = None;
    let build_proof = json!({"command_proof":{"command":"cargo test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let contract_proof =
        json!({"contract_proof":{"seam_test_ref":"tests/acdd/mcp.rs","red_exit_code":101}});
    let delivery_proof = json!({"delivery_proof":{"status":"passed"}});
    let contours: serde_json::Map<String, Value> = REVIEW_CONTOURS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                json!({"applicable":true,"evidence":"independent fixture review"}),
            )
        })
        .collect();
    let review_proof = json!({"review_proof":{"decision":"pass","contours":contours}});
    for (gate, stage) in GATES.iter().enumerate() {
        let workspace = if gate == 2 { &reviewer } else { &builder };
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let claim = client.payload(
            "task_claim",
            json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":workspace.0}),
        );
        if gate == 0 {
            let bundle = &claim["context_bundle"];
            assert!(
                bundle.is_object(),
                "omitted MCP bundle option returns context"
            );
            assert_eq!(bundle["contract"]["task_id"], id);
            assert_eq!(bundle["contract"]["stage"], *stage);
            assert!(bundle["guidance"]["actionable_steps"]
                .as_array()
                .is_some_and(|steps| !steps.is_empty()));
        }
        let proof = if gate == 3 {
            &delivery_proof
        } else if gate == 2 {
            &review_proof
        } else if gate == 0 {
            &contract_proof
        } else {
            &build_proof
        };
        let evidence = json!({"task_id":id,"stage":stage,"claim_revision":claim["claim_revision"],"contract_revision":1,"worker_id":worker,"worktree":workspace.0,"proof":proof});
        if gate == 0 {
            for invalid in [
                json!({"task_id":id,"stage":stage,"action":"pass","evidence_ref":"proof.yaml"}),
                json!({"task_id":id,"stage":stage,"action":"pass","evidence":42}),
            ] {
                let (_, rejected) = client.call("task_submit", invalid);
                assert!(rejected.get("error").is_some() || rejected["result"]["isError"] == true);
            }
        }
        let args = json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"});
        if gate == 3 {
            builder.write(
                "docs/milestones/010-tasks.md",
                &source
                    .replace("target: Deliver", "target: Unadmitted change")
                    .replace('\n', "\r\n"),
            );
            let (_, unadmitted) = client.call("task_submit", args.clone());
            assert_eq!(unadmitted["result"]["isError"], true);
            builder.write(
                "docs/milestones/010-tasks.md",
                &source.replace('\n', "\r\n"),
            );
        }
        let submitted = client.payload("task_submit", args.clone());
        let snapshot = submitted["snapshot"]["commit"].as_str().unwrap();
        if gate == 1 {
            candidate_commit = Some(snapshot.to_owned());
        }
        if gate == 3 {
            delivery_commit = Some(snapshot.to_owned());
            assert_eq!(submitted["receipt"]["commit"], snapshot);
        } else {
            assert_eq!(
                submitted["snapshot"]["inspect_cmd"],
                contextunity_forge_mcp::core::tasks::snapshot_inspect_cmd(snapshot)
            );
        }
        assert_eq!(
            submitted["status"],
            if gate == 3 { "completed" } else { "ready" }
        );
        assert_eq!(client.payload("task_submit", args.clone()), submitted);
        if gate == 3 {
            let (_, response) = client.call("task_submit", args);
            let markdown = response["result"]["content"][0]["text"].as_str().unwrap();
            assert!(markdown.starts_with("# Task submission"));
            assert!(markdown.contains(id));
            assert!(markdown.contains("completed"));
            assert!(markdown.contains("Durable receipt"));
            assert!(markdown.contains(submitted["receipt"]["commit"].as_str().unwrap()));
        }
    }
    assert_eq!(client.payload("task_list", json!({}))["tasks"], json!([]));
    assert_eq!(
        client.payload("task_list", json!({"status":"completed"}))["tasks"][0]["task_id"],
        id
    );
    let written = fs::read_to_string(builder.0.join("docs/milestones/010-tasks.md")).unwrap();
    let milestone =
        contextunity_forge_mcp::core::tasks::Milestone::parse(&written, "contextunity").unwrap();
    assert!(milestone.tasks[0]
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .is_none());
    let stored = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    assert_eq!(
        stored["latest_snapshot"]["commit"],
        candidate_commit.unwrap()
    );
    assert_eq!(stored["receipt"]["commit"], delivery_commit.unwrap());
    assert_eq!(milestone.deferred_defects.len(), 1);
    let defect = &milestone.deferred_defects[0];
    assert_eq!(defect.id, "DEFECT-E2E-001");
    assert_eq!(defect.source, "review_findings");
    assert_eq!(defect.title, "Typed defect survives the MCP task lifecycle");
    assert_eq!(defect.path.as_deref(), Some("src/engine/tasks.rs"));
    assert_eq!(defect.disposition, "subsequent_milestone");
    assert_eq!(
        defect.notes.as_deref(),
        Some("Preserved through receipt rewriting")
    );
    assert_eq!(milestone.tasks[0].status.as_deref(), Some("completed"));
}

#[test]
fn stdio_tool_catalog_uses_object_schemas_for_every_property() {
    let workspace = Workspace::new();
    let mut client = Client::new(&workspace);
    let (_, response) = client.request("tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 20);
    for tool in tools {
        assert!(
            tool["inputSchema"]["properties"]["worktree"].is_object(),
            "{} does not advertise worktree selection",
            tool["name"]
        );
        for (name, schema) in tool["inputSchema"]["properties"].as_object().unwrap() {
            assert!(schema.is_object(), "{}.{name}: {schema}", tool["name"]);
        }
    }
    let names: Vec<_> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| n.starts_with("task_"))
        .collect();
    assert_eq!(names.len(), 5);
    for name in [
        "task_list",
        "task_claim",
        "task_submit",
        "task_manage",
        "task_blackboard",
    ] {
        assert!(names.contains(&name));
        let schema = &tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"];
        assert_eq!(schema["additionalProperties"], false);
    }
    let content = &tools
        .iter()
        .find(|tool| tool["name"] == "session_checkpoint")
        .unwrap()["inputSchema"]["properties"]["content"];
    assert!(content.is_object());
    for value in [
        json!(true),
        json!(42),
        json!("text"),
        json!([1]),
        json!({"key":1}),
    ] {
        client.payload(
            "session_checkpoint",
            json!({"action":"save","name":"sample","content":value}),
        );
        assert_eq!(
            client.payload(
                "session_checkpoint",
                json!({"action":"get","name":"sample"})
            ),
            value
        );
    }
}

#[test]
fn mcp_task_claim_guidance_includes_rules_without_schema_bloat() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write(
        "docs/milestones/010-guidance.md",
        "---\nid: m-guidance\ntitle: Claim guidance\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: full\ntarget: Check full claim guidance\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: minimal\ntarget: Check minimal claim guidance\nproof_policy: seam-test-first\nscope: [tests/]\n```\n",
    );

    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-guidance.md"}),
    );
    let tasks = synced["tasks"].as_array().unwrap();
    let full_id = tasks[0]["task_id"].as_str().unwrap();
    let minimal_id = tasks[1]["task_id"].as_str().unwrap();
    let worktree = workspace.0.to_str().unwrap();
    let full_claim = client.payload(
        "task_claim",
        json!({"task_id":full_id,"stage":"contract","worker_id":"full-builder","worktree":worktree}),
    );
    let minimal_claim = client.payload(
        "task_claim",
        json!({"task_id":minimal_id,"stage":"contract","worker_id":"minimal-builder","worktree":worktree,"bundle":false}),
    );

    const PROTECTED_RULE: &str = "PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile are immutable while tasks are active. Declare task invariants inside task.invariants.";
    let expected_rules = [
        format!(
            "ISOLATION: Develop exclusively inside dedicated worktree '{}'.",
            workspace.0.display()
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

    let (_, catalog) = client.request("tools/list", json!({}));
    let catalog_text = serde_json::to_string(&catalog).unwrap();
    assert!(
        serde_json::to_vec(&catalog).unwrap().len() <= 65_536,
        "MCP tool catalog exceeded the 64 KiB response budget"
    );
    assert!(!catalog_text.contains(PROTECTED_RULE));
    for rule_prefix in ["ISOLATION:", "IMMUTABILITY:", "INDEPENDENCE:"] {
        assert!(
            !catalog_text.contains(rule_prefix),
            "runtime claim guidance must not be duplicated in tool schema metadata: {rule_prefix}"
        );
    }
}

#[test]
fn task_mcp_and_cli_share_ready_claim_reset_selectors_and_diagnostic() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write("docs/milestones/010-tasks.md","---\nid: m-pilot\ntitle: Pilot\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n");
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md"}),
    );
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let ready = client.payload("task_list", json!({}));
    assert_eq!(ready["tasks"].as_array().unwrap().len(), 1);
    let cli = json_cli()
        .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(serde_json::from_slice::<Value>(&cli.stdout).unwrap(), ready);
    let args = json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":workspace.0,"format":"json"});
    let claimed = client.payload("task_claim", args.clone());
    let direct_error = contextunity_forge_mcp::engine::tasks::claim(
        &workspace.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: id.to_owned(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: workspace.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    let (_, collision) = client.call("task_claim", args);
    assert_eq!(collision["result"]["isError"], true);
    let error: Value =
        serde_json::from_str(collision["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "TASK_ALREADY_CLAIMED");
    assert_eq!(error["error"]["message"], direct_error);
    assert_eq!(client.payload("task_list", json!({}))["tasks"], json!([]));
    let active = client.payload("task_list", json!({"status":"in_progress"}));
    assert_eq!(active["tasks"][0]["task_id"], id);
    assert_eq!(active["tasks"][0]["owner"], "builder");
    assert_eq!(active["tasks"][0]["stage"], "contract");
    let reset = json_cli()
        .args(["--root", workspace.0.to_str().unwrap(), "task", "reset", id])
        .output()
        .unwrap();
    assert!(reset.status.success());
    let reset: Value = serde_json::from_slice(&reset.stdout).unwrap();
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
        let (_, response) = client.call(name, args);
        assert!(response["result"]["isError"] == true || response.get("error").is_some());
    }
}

#[test]
fn task_blackboard_mcp_and_cli_share_sqlite_messages() {
    use contextunity_forge_mcp::db::tasks_store::TasksStore;

    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md",
        "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\n---\n```yaml\ntask_ref: first\ntarget: Coordinate agents\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"}),
    );
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();

    let (_, mcp_post) = client.call(
        "task_blackboard",
        json!({"action":"post","task_id":task_id,"topic":"draft","payload":"{\"red\":true}","format":"json"}),
    );
    let cli_post = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "post",
            task_id,
            "--topic",
            "notes",
            "--payload",
            "cargo test --all-targets passed",
        ])
        .output()
        .unwrap();
    assert!(
        mcp_post.get("error").is_none()
            && mcp_post["result"]["isError"] != true
            && cli_post.status.success(),
        "MCP: {mcp_post}; CLI: {}",
        String::from_utf8_lossy(&cli_post.stderr)
    );
    let mcp_post: Value =
        serde_json::from_str(mcp_post["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let cli_post: Value = serde_json::from_slice(&cli_post.stdout).unwrap();
    let mcp_read = client.payload(
        "task_blackboard",
        json!({"action":"read","task_id":task_id,"limit":2}),
    );
    assert_eq!(mcp_read["messages"].as_array().unwrap().len(), 2);
    assert_eq!(mcp_read["messages"][0]["id"], cli_post["id"]);
    assert_eq!(mcp_read["messages"][1]["id"], mcp_post["id"]);

    let cli_read = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            task_id,
            "--topic",
            "draft",
        ])
        .output()
        .unwrap();
    assert!(
        cli_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_read.stderr)
    );
    let cli_read: Value = serde_json::from_slice(&cli_read.stdout).unwrap();
    assert_eq!(cli_read["messages"].as_array().unwrap().len(), 1);
    assert_eq!(cli_read["messages"][0]["id"], mcp_post["id"]);

    let (_, mcp_markdown) = client.call(
        "task_blackboard",
        json!({"action":"read","task_id":task_id,"topic":"draft","format":"markdown"}),
    );
    let mcp_markdown = mcp_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let cli_markdown = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--format",
            "markdown",
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            task_id,
            "--topic",
            "draft",
        ])
        .output()
        .unwrap();
    assert!(
        cli_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_markdown.stderr)
    );
    let cli_markdown = std::str::from_utf8(&cli_markdown.stdout).unwrap();
    assert_eq!(mcp_markdown.trim_end(), cli_markdown.trim_end());
    assert!(mcp_markdown.contains("## draft"));
    assert!(mcp_markdown.contains("task_blackboard(action: \"inspect\", message_id:"));

    let store = TasksStore::open(&workspace.0.join("tasks.sqlite")).unwrap();
    let stored = store.blackboard_read(task_id, None, None).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].payload.as_deref(), Some("{\"red\":true}"));
    assert_eq!(
        stored[1].payload.as_deref(),
        Some("cargo test --all-targets passed")
    );
    assert_eq!(stored[0].author, "mcp");
    assert_eq!(stored[1].author, "cli");

    let mcp_list = client.payload("task_list", json!({"milestone_ref":"first","status":"all"}));
    assert_eq!(mcp_list["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(mcp_list["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_list["resolution"]["input"], "first");
    assert_eq!(mcp_list["resolution"]["task_id"], task_id);
    assert_eq!(mcp_list["resolution"]["milestone_id"], "m-blackboard");
    assert_eq!(
        mcp_list["resolution"]["milestone_ref"],
        "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    let mcp_alias_list = client.payload(
        "task_list",
        json!({"milestone_ref":"UX-001","status":"all"}),
    );
    let cli_alias_list = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone",
            "UX-001",
            "--status",
            "all",
        ])
        .output()
        .unwrap();
    assert!(
        cli_alias_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_alias_list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_alias_list.stdout).unwrap(),
        mcp_alias_list
    );
    assert_eq!(mcp_alias_list["tasks"].as_array().unwrap().len(), 1);
    assert!(mcp_alias_list.get("resolution").is_none());
    let cli_list = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone",
            "first",
            "--status",
            "all",
        ])
        .output()
        .unwrap();
    assert!(
        cli_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_list.stdout).unwrap(),
        mcp_list
    );

    let (_, mcp_list_markdown) = client.call(
        "task_list",
        json!({"milestone_ref":"first","status":"all","format":"markdown"}),
    );
    let mcp_list_markdown = mcp_list_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let cli_list_markdown = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--format",
            "markdown",
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone",
            "first",
            "--status",
            "all",
        ])
        .output()
        .unwrap();
    assert!(
        cli_list_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list_markdown.stderr)
    );
    let cli_list_markdown = std::str::from_utf8(&cli_list_markdown.stdout).unwrap();
    assert_eq!(mcp_list_markdown.trim_end(), cli_list_markdown.trim_end());
    assert!(
        mcp_list_markdown
            .contains("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
    );
    assert_eq!(
        mcp_list_markdown
            .matches("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
            .count(),
        1,
        "task-list Markdown displays fallback provenance exactly once"
    );

    let canonical_milestone = store
        .milestone_scope_ref(
            "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md",
        )
        .unwrap();
    let milestone_message_count = || {
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM task_blackboard WHERE milestone_ref = ?1 AND task_id IS NULL AND subtask_ref IS NULL",
                [&canonical_milestone],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    let before_fallback_posts = milestone_message_count();
    let mcp_alias_read = client.payload(
        "task_blackboard",
        json!({
            "action":"read",
            "scope":"milestone",
            "milestone_ref":"UX-001",
            "topic":"decisions"
        }),
    );
    let cli_alias_read = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "UX-001",
            "--topic",
            "decisions",
        ])
        .output()
        .unwrap();
    assert!(
        cli_alias_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_alias_read.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_alias_read.stdout).unwrap(),
        mcp_alias_read
    );
    assert!(mcp_alias_read["messages"].as_array().unwrap().is_empty());
    assert!(mcp_alias_read.get("resolution").is_none());

    let (_, mcp_fallback_post) = client.call(
        "task_blackboard",
        json!({
            "action":"post",
            "scope":"milestone",
            "milestone_ref":"first",
            "topic":"decisions",
            "payload":"MCP fallback post",
            "format":"json"
        }),
    );
    assert_ne!(mcp_fallback_post["result"]["isError"], true);
    let mcp_fallback_post: Value = serde_json::from_str(
        mcp_fallback_post["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mcp_fallback_post["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_fallback_post["resolution"]["input"], "first");
    assert_eq!(mcp_fallback_post["resolution"]["task_id"], task_id);
    assert_eq!(
        mcp_fallback_post["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(
        mcp_fallback_post["resolution"]["milestone_ref"],
        "forge-mcp/forge-mcp/docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    assert_eq!(milestone_message_count(), before_fallback_posts + 1);

    let cli_fallback_post = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "post",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
            "--payload",
            "CLI fallback post",
        ])
        .output()
        .unwrap();
    assert!(
        cli_fallback_post.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_post.stderr)
    );
    let cli_fallback_post: Value = serde_json::from_slice(&cli_fallback_post.stdout).unwrap();
    assert_eq!(cli_fallback_post["resolution"]["kind"], "task_parent");
    assert_eq!(cli_fallback_post["resolution"]["input"], "first");
    assert_eq!(cli_fallback_post["resolution"]["task_id"], task_id);
    assert_eq!(
        cli_fallback_post["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(
        cli_fallback_post["resolution"]["milestone_ref"],
        "forge-mcp/forge-mcp/docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    assert_ne!(cli_fallback_post["id"], mcp_fallback_post["id"]);
    assert_eq!(milestone_message_count(), before_fallback_posts + 2);

    let mcp_fallback_read = client.payload(
        "task_blackboard",
        json!({
            "action":"read",
            "scope":"milestone",
            "milestone_ref":"first",
            "topic":"decisions"
        }),
    );
    let cli_fallback_read = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
        ])
        .output()
        .unwrap();
    assert!(
        cli_fallback_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_read.stderr)
    );
    let cli_fallback_read: Value = serde_json::from_slice(&cli_fallback_read.stdout).unwrap();
    assert_eq!(cli_fallback_read, mcp_fallback_read);
    assert_eq!(mcp_fallback_read["messages"].as_array().unwrap().len(), 2);
    assert_eq!(mcp_fallback_read["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_fallback_read["resolution"]["input"], "first");
    assert_eq!(mcp_fallback_read["resolution"]["task_id"], task_id);
    assert_eq!(
        mcp_fallback_read["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(milestone_message_count(), before_fallback_posts + 2);

    let (_, mcp_fallback_markdown) = client.call(
        "task_blackboard",
        json!({
            "action":"read",
            "scope":"milestone",
            "milestone_ref":"first",
            "topic":"decisions",
            "format":"markdown"
        }),
    );
    let mcp_fallback_markdown = mcp_fallback_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let cli_fallback_markdown = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--format",
            "markdown",
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
        ])
        .output()
        .unwrap();
    assert!(
        cli_fallback_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_markdown.stderr)
    );
    let cli_fallback_markdown = std::str::from_utf8(&cli_fallback_markdown.stdout).unwrap();
    assert_eq!(
        mcp_fallback_markdown.trim_end(),
        cli_fallback_markdown.trim_end()
    );
    assert!(
        mcp_fallback_markdown
            .contains("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
    );
    assert_eq!(
        mcp_fallback_markdown
            .matches("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
            .count(),
        1,
        "blackboard Markdown displays fallback provenance exactly once"
    );
}

#[test]
fn milestone_blackboard_lists_are_bounded_payload_free_and_inspectable() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/010-blackboard.md",
        "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n",
    );
    let mut client = Client::new(&workspace);

    let mut message_id = 0;
    let mut blocker_id = 0;
    let mut posted_ids = Vec::new();
    let blocker_body = "blocker needs review\nsecond body line";
    for index in 0..12 {
        let is_blocker = index == 10;
        let (_, response) = client.call(
            "task_blackboard",
            json!({
                "action":"post",
                "topic":if is_blocker {"blockers"} else if index == 9 {"notes"} else {"decisions"},
                "author":if is_blocker {"fixture-builder"} else {"fixture-writer"},
                "gate":if is_blocker {Some("build")} else {None},
                "payload":if is_blocker {blocker_body.to_owned()} else {format!("milestone note {index}")},
                "format":"json"
            }),
        );
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "{response}"
        );
        let posted: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        message_id = posted["id"].as_u64().unwrap();
        posted_ids.push(message_id);
        if is_blocker {
            blocker_id = message_id;
        }
    }

    let messages = client.payload("task_blackboard", json!({"action":"read"}));
    assert_eq!(messages["messages"].as_array().unwrap().len(), 10);
    assert_eq!(messages["pagination"]["limit"], 10);
    assert_eq!(messages["pagination"]["offset"], 0);
    assert_eq!(messages["pagination"]["has_more"], true);
    assert_eq!(messages["pagination"]["next_offset"], 10);
    assert_eq!(messages["messages"][0]["id"], message_id);
    assert_eq!(messages["messages"][0]["task_id"], Value::Null);
    assert!(messages["messages"][0].get("payload").is_none());
    assert_eq!(messages["messages"][1]["id"], blocker_id);
    assert_eq!(messages["messages"][1]["topic"], "blockers");
    assert_eq!(messages["messages"][1]["author"], "fixture-builder");
    assert!(messages["messages"][1]["milestone_ref"]
        .as_str()
        .unwrap()
        .ends_with("docs/milestones/010-blackboard.md"));
    assert!(messages["messages"][1]["task_id"].is_null());
    assert!(messages["messages"][1]["subtask_ref"].is_null());
    assert_eq!(messages["messages"][1]["gate"], "build");
    assert!(messages["messages"][1]["created_at"].is_i64());
    assert!(messages["messages"][1].get("payload").is_none());
    assert!(messages["messages"]
        .as_array()
        .unwrap()
        .windows(2)
        .all(|pair| { pair[0]["id"].as_u64().unwrap() > pair[1]["id"].as_u64().unwrap() }));

    let (_, listed_markdown) = client.call(
        "task_blackboard",
        json!({"action":"read","limit":10,"format":"markdown"}),
    );
    let listed_markdown = listed_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(listed_markdown.contains(&format!(
        "[!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
    )));
    let listed_lowercase = listed_markdown.to_lowercase();
    assert!(listed_lowercase.contains("scope") && listed_lowercase.contains("milestone"));
    assert!(listed_lowercase.contains("gate") && listed_lowercase.contains("build"));
    let created_at = messages["messages"][1]["created_at"].as_i64().unwrap();
    assert!(listed_markdown.contains(&created_at.to_string()));
    let listed_blocker = listed_markdown
        .split(&format!(
            "> [!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
        ))
        .nth(1)
        .and_then(|section| section.split("\n\n").next())
        .unwrap();
    assert!(listed_blocker.contains(&format!(
        "> Scope: `milestone`\n> Milestone ref: `{}`",
        messages["messages"][1]["milestone_ref"].as_str().unwrap()
    )));
    assert!(listed_blocker.contains(&format!("> Timestamp: `{created_at}`")));
    assert!(listed_markdown.contains(&format!(
        "task_blackboard(action: \"inspect\", message_id: {blocker_id})"
    )));
    assert!(listed_markdown.contains("Body not included in this message summary."));
    assert!(!listed_markdown.contains("> Body: \"\""));
    assert!(!listed_markdown.contains("second body line"));
    assert_eq!(listed_markdown.matches("[!CAUTION] BLOCKER").count(), 1);
    assert!(listed_markdown.contains("## Pagination"));
    assert!(listed_markdown.contains("**next_offset**: `10`"));
    assert!(listed_markdown.contains("**has_more**: `true`"));
    assert!(listed_markdown.contains("## decisions"));
    assert!(listed_markdown.contains("## notes"));
    let newest_decision = listed_markdown
        .find(&format!("### Message #{}", posted_ids[11]))
        .unwrap();
    let older_decision = listed_markdown
        .find(&format!("### Message #{}", posted_ids[8]))
        .unwrap();
    assert!(newest_decision < older_decision);

    let second_page = client.payload(
        "task_blackboard",
        json!({"action":"read","limit":10,"offset":messages["pagination"]["next_offset"]}),
    );
    assert_eq!(second_page["messages"].as_array().unwrap().len(), 2);
    assert_eq!(second_page["messages"][0]["id"], posted_ids[1]);
    assert_eq!(second_page["messages"][1]["id"], posted_ids[0]);
    assert_eq!(second_page["pagination"]["offset"], 10);
    assert_eq!(second_page["pagination"]["has_more"], false);
    let (_, second_page_markdown) = client.call(
        "task_blackboard",
        json!({"action":"read","limit":10,"offset":messages["pagination"]["next_offset"],"format":"markdown"}),
    );
    let second_page_markdown = second_page_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(second_page_markdown.contains("**has_more**: `false`"));
    assert!(second_page_markdown.contains("**offset**: `10`"));
    assert!(second_page_markdown.contains("**next_offset**: `null`"));
    assert!(!second_page_markdown.contains("[!CAUTION]"));
    assert!(second_page_markdown.contains(&format!("### Message #{}", posted_ids[1])));
    assert!(second_page_markdown.contains(&format!("### Message #{}", posted_ids[0])));

    let inspected = client.payload(
        "task_blackboard",
        json!({"action":"inspect","message_id":message_id}),
    );
    assert_eq!(inspected["message"]["id"], message_id);
    assert_eq!(inspected["message"]["payload"], "milestone note 11");

    let (_, blocker_inspect) = client.call(
        "task_blackboard",
        json!({"action":"inspect","message_id":blocker_id,"format":"markdown"}),
    );
    let blocker_inspect = blocker_inspect["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(blocker_inspect.contains(&format!(
        "[!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
    )));
    let blocker_inspect_lowercase = blocker_inspect.to_lowercase();
    assert!(
        blocker_inspect_lowercase.contains("scope")
            && blocker_inspect_lowercase.contains("milestone")
    );
    assert!(
        blocker_inspect_lowercase.contains("gate") && blocker_inspect_lowercase.contains("build")
    );
    assert!(blocker_inspect.contains("second body line"));

    let cli_inspect = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "inspect",
            &message_id.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        cli_inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_inspect.stderr)
    );
    let cli_inspect: Value = serde_json::from_slice(&cli_inspect.stdout).unwrap();
    assert_eq!(cli_inspect["message"]["id"], message_id);
    assert_eq!(cli_inspect["message"]["payload"], "milestone note 11");

    let empty_blocker = client.payload(
        "task_blackboard",
        json!({
            "action":"post",
            "topic":"blockers",
            "author":"fixture-builder",
            "gate":"build",
            "payload":""
        }),
    );
    let empty_blocker_id = empty_blocker["id"].as_u64().unwrap();
    let (_, empty_blocker_response) = client.call(
        "task_blackboard",
        json!({"action":"inspect","message_id":empty_blocker_id,"format":"markdown"}),
    );
    let empty_blocker_markdown = empty_blocker_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(empty_blocker_markdown.contains("> Body: \"\""));
    assert!(!empty_blocker_markdown.contains(&format!(
        "task_blackboard(action: \"inspect\", message_id: {empty_blocker_id})"
    )));
}

#[test]
fn subtask_mcp_and_cli_operations() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-mcp-subtasks\ntitle: MCP Subtasks\ndoc_type: contract\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: sub-demo\ntarget: Subtask demonstration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    workspace.write("docs/milestones/010-subtasks.md", manifest);
    workspace.write("AGENTS.md", "# Agent Rules\n");
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    let mut client = Client::new(&workspace);
    client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-subtasks.md"}),
    );
    let task_id = "forge-mcp/m-mcp-subtasks:sub-demo";

    // MCP task_manage subtask_add
    let add_res = client.payload(
        "task_manage",
        json!({
            "action": "subtask_add",
            "task_id": task_id,
            "subtask_ref": "step-1",
            "title": "Initial exploration"
        }),
    );
    assert_eq!(add_res["subtask"]["subtask_ref"], "step-1");
    assert_eq!(add_res["subtask"]["status"], "ready");

    // MCP task_manage subtask_update
    let update_res = client.payload(
        "task_manage",
        json!({
            "action": "subtask_update",
            "task_id": task_id,
            "subtask_ref": "step-1",
            "subtask_status": "completed",
            "evidence": "Explored symbols"
        }),
    );
    assert_eq!(update_res["subtask"]["status"], "completed");
    assert_eq!(update_res["subtask"]["evidence"], "Explored symbols");

    for (input, expected) in [
        ("ready", "ready"),
        ("in_progress", "in_progress"),
        ("cancelled", "cancelled"),
        ("blocked", "blocked"),
        ("pending", "ready"),
    ] {
        let updated = client.payload(
            "task_manage",
            json!({
                "action": "subtask_update", "task_id": task_id,
                "subtask_ref": "step-1", "subtask_status": input,
            }),
        );
        assert_eq!(updated["subtask"]["status"], expected);
        assert_eq!(updated["subtask"]["evidence"], "Explored symbols");
    }

    // CLI task subtask add
    let cli_add = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "subtask",
            "add",
            task_id,
            "step-2",
            "Run unit test",
        ])
        .output()
        .unwrap();
    assert!(
        cli_add.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_add.stderr)
    );
    let cli_add_json: Value = serde_json::from_slice(&cli_add.stdout).unwrap();
    assert_eq!(cli_add_json["subtask"]["subtask_ref"], "step-2");
    assert_eq!(cli_add_json["subtask"]["status"], "ready");
    let legacy_update = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "subtask",
            "update",
            task_id,
            "step-2",
            "--status",
            "pending",
        ])
        .output()
        .unwrap();
    assert!(
        legacy_update.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy_update.stderr)
    );
    let legacy_json: Value = serde_json::from_slice(&legacy_update.stdout).unwrap();
    assert_eq!(legacy_json["subtask"]["status"], "ready");

    // CLI task subtask list
    let cli_list = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "subtask",
            "list",
            task_id,
        ])
        .output()
        .unwrap();
    assert!(
        cli_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list.stderr)
    );
    let cli_list_json: Value = serde_json::from_slice(&cli_list.stdout).unwrap();
    assert_eq!(cli_list_json["subtasks"].as_array().unwrap().len(), 2);

    // MCP task_manage inspect returns subtasks
    let inspected = client.payload(
        "task_manage",
        json!({"action": "inspect", "task_id": task_id}),
    );
    assert_eq!(inspected["subtasks"].as_array().unwrap().len(), 2);

    workspace.write(
        "docs/milestones/010-subtasks.md",
        &manifest.replace(
            "task_ref: sub-demo",
            "task_ref: sub-demo\nstatus: cancelled",
        ),
    );
    client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-subtasks.md"}),
    );
    let cancelled = client.payload(
        "task_list",
        json!({"status": "cancelled", "milestone_ref": "docs/milestones/010-subtasks.md"}),
    );
    assert_eq!(cancelled["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(cancelled["tasks"][0]["status"], "cancelled");
    let idle = client.payload(
        "task_manage",
        json!({"action": "next", "worker_id": "worker", "worktree": workspace.0.to_str().unwrap()}),
    );
    assert_eq!(idle["status"], "idle");
    let cli_cancelled = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone",
            "010",
            "--status",
            "cancelled",
        ])
        .output()
        .unwrap();
    assert!(
        cli_cancelled.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_cancelled.stderr)
    );
    let cancelled_json: Value = serde_json::from_slice(&cli_cancelled.stdout).unwrap();
    assert_eq!(cancelled_json["tasks"][0]["status"], "cancelled");
    let (_, cancelled_response) = client.call(
        "task_list",
        json!({"status":"cancelled","milestone_ref":"docs/milestones/010-subtasks.md"}),
    );
    let cancelled_markdown = cancelled_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(cancelled_markdown.contains(task_id));
    assert!(cancelled_markdown.contains("cancelled"));
    let cli_cancelled_markdown = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone",
            "010",
            "--status",
            "cancelled",
        ])
        .output()
        .unwrap();
    assert!(
        cli_cancelled_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_cancelled_markdown.stderr)
    );
    let cli_cancelled_markdown = String::from_utf8(cli_cancelled_markdown.stdout).unwrap();
    assert!(cli_cancelled_markdown.contains(task_id));
    assert!(cli_cancelled_markdown.contains("cancelled"));
}

#[test]
fn task_claim_bundle_and_task_manage_context_returns_unified_agent_context() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("tests")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/adr")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();

    workspace.write("AGENTS.md", "# Test guidance\n");
    let routing_adr = "---\ntitle: Task Routing Invariants\ndoc_type: adr\nstatus: accepted\n---\n# Task Routing Invariants\n- INV-ADR: Preserve [`route_task`](../../src/routing.rs#L2).\n- Decision: Admit every caller before changing the public signature.";
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
    workspace.write(
        "src/routing.rs",
        "pub struct Router;\npub fn route_task() {}\n",
    );
    workspace.write("src/unique.rs", "pub fn unique() {}\n");
    let mut decision_rules = String::from("---\ntitle: Decision after rules\ndoc_type: adr\nstatus: accepted\n---\n# Decision after rules\n");
    for index in 0..6 {
        decision_rules.push_str(&format!("- INV-DEC{index:02}: Preserve decision rule {index} through [`unique`](../../src/unique.rs#L1).\n"));
    }
    decision_rules
        .push_str("Decision: Keep the complete decision after every bounded navigation rule.\n");
    workspace.write("docs/adr/003-decision-after-rules.md", &decision_rules);
    workspace.write("docs/adr/004-headed-decision.md", "---\ntitle: Headed Decision\ndoc_type: adr\nstatus: accepted\n---\n# Governance\n- INV-HEADED: Preserve the decision through [`unique`](../../src/unique.rs#L1).\n## Decision\nRetain the complete decision from its own section.\n");
    workspace.write("src/minimal_routing.rs", "pub fn minimal_route_task() {}\n");
    workspace.write(
        "src/minimal_markdown.rs",
        "pub fn minimal_markdown_task() {}\n",
    );
    workspace.write("tests/test_routing.rs", "#[test]\nfn test_routing() {}\n");
    workspace.write("src/cli_routing.rs", "pub fn route_cli_task() {}\n");
    workspace.write(
        "tests/test_cli_routing.rs",
        "#[test]\nfn test_cli_routing() {}\n",
    );

    let manifest = "---\nid: m-context\ntitle: Context Milestone\ndoc_type: contract\ninvariants: [routing-invariant]\n---\n```yaml\ntask_ref: routing-task-with-a-long-identity-for-blocker-regression\ntarget: Deliver unified routing\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs\n  - tests/test_routing.rs\n```\n```yaml\ntask_ref: cli-routing-task\ntarget: Deliver CLI unified routing\nproof_policy: seam-test-first\nscope:\n  - src/cli_routing.rs\n  - tests/test_cli_routing.rs\n```\n```yaml\ntask_ref: minimal-routing-task\ntarget: Deliver minimal routing\nproof_policy: seam-test-first\nscope:\n  - src/minimal_routing.rs\n```\n";
    let manifest = format!("{manifest}```yaml\ntask_ref: decision-context-task\ntarget: Preserve governing ADR decisions\nproof_policy: seam-test-first\nscope:\n  - src/unique.rs\n```\n");
    let manifest = format!("{manifest}```yaml\ntask_ref: dependency-context-task\ntarget: Preserve declared task dependencies\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs\ndepends_on: [routing-task-with-a-long-identity-for-blocker-regression]\n```\n");
    let manifest = format!("{manifest}```yaml\ntask_ref: minimal-markdown-context-task\ntarget: Render a minimal claim context\nproof_policy: seam-test-first\nscope:\n  - src/minimal_markdown.rs\n```\n");
    let manifest = manifest.replace("invariants: [routing-invariant]", "invariants: [routing-invariant, authority-2, authority-3, authority-4, authority-5, authority-6, authority-7]");
    workspace.write(
        "docs/milestones/010-context.md",
        &manifest.replace(
            "task_ref: minimal-routing-task\ntarget: Deliver minimal routing\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs",
            "task_ref: minimal-routing-task\ntarget: Deliver minimal routing\nproof_policy: seam-test-first\nscope:\n  - src/minimal_routing.rs",
        ),
    );
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    contextunity_forge_mcp::db::writer::build(
        &workspace.0,
        &workspace.0.join(".forge/code-map.sqlite"),
        None,
    )
    .unwrap();

    let mut client = Client::new(&workspace);
    let sync_res = client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-context.md"}),
    );
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();
    let cli_task_id = sync_res["tasks"][1]["task_id"].as_str().unwrap();
    let minimal_task_id = sync_res["tasks"][2]["task_id"].as_str().unwrap();
    let decision_task_id = sync_res["tasks"][3]["task_id"].as_str().unwrap();
    let dependency_task_id = sync_res["tasks"][4]["task_id"].as_str().unwrap();
    let minimal_markdown_task_id = sync_res["tasks"][5]["task_id"].as_str().unwrap();

    // Post blackboard message
    let decision_post = client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "task_id": task_id,
            "topic": "decisions",
            "payload": "Route tasks through unified zero-shot context bundle",
            "author": "architect"
        }),
    );
    client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "task_id": cli_task_id,
            "topic": "notes",
            "payload": "Sibling task context is milestone scoped",
            "author": "cli-author"
        }),
    );

    // 1. MCP task_manage action: "context"
    let context_res = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
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
    assert!(bundle["guidance"]["recommended_tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty()));
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
        1,
        "unrelated ADRs must not appear as governing context"
    );
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
    assert!(symbols.iter().any(|symbol| symbol["name"] == "route_task"));

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
    let claim_res = client.payload(
        "task_claim",
        json!({
            "task_id": task_id,
            "stage": "contract",
            "worker_id": "author-1",
            "worktree": workspace.0
        }),
    );
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
    let cli_claim = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "claim",
            cli_task_id,
            "--stage",
            "contract",
            "--worker",
            "cli-author",
            "--worktree",
            workspace.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
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
    let cli_context = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "context",
            task_id,
        ])
        .output()
        .unwrap();
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
    let minimal_claim = client.payload(
        "task_claim",
        json!({
            "task_id": minimal_task_id,
            "stage": "contract",
            "worker_id": "author-minimal",
            "worktree": workspace.0,
            "bundle": false
        }),
    );
    assert!(minimal_claim.get("context_bundle").is_none());
    assert!(minimal_claim.get("gates").is_none());
    assert!(minimal_claim.get("attempts").is_none());
    assert!(minimal_claim.get("findings").is_none());
    assert!(minimal_claim.get("receipt").is_none());
    assert!(minimal_claim.get("spec").is_none());
    assert_eq!(minimal_claim["depends_on"], json!([]));
    assert!(minimal_claim["subtasks"].is_array());
    assert!(minimal_claim["workflow_guidance"].is_object());
    assert!(minimal_claim["allowed_write_scope"].is_array());
    assert_eq!(minimal_claim["task_id"], minimal_task_id);

    // 6. Milestone-scoped blackboard message (task_id IS NULL) with multi-byte Ukrainian text > 500 chars
    let ukr_payload = "Тестовий запис архітектури українською мовою для перевірки безпечного обрізання символів UTF-8. ".repeat(10);
    client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "milestone_ref": "docs/milestones/010-context.md",
            "topic": "notes",
            "payload": ukr_payload,
            "author": "ukr-architect"
        }),
    );
    let context_after = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
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
        .find(|m| m["author"] == "cli-author")
        .unwrap();
    assert_eq!(sibling_msg["scope"], "sibling");
    assert_eq!(sibling_msg["task_id"], cli_task_id);

    let decision_context = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": decision_task_id}),
    );
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
    let cli_decision = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "context",
            decision_task_id,
        ])
        .output()
        .unwrap();
    if cli_decision.status.success() {
        let cli_decision: Value = serde_json::from_slice(&cli_decision.stdout).unwrap();
        for path in [
            "docs/adr/003-decision-after-rules.md",
            "docs/adr/004-headed-decision.md",
        ] {
            if !cli_decision["context_bundle"]["adrs"]
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
                    cli_decision["context_bundle"]["adrs"]
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
    let blocker_post = client.payload(
        "task_blackboard",
        json!({
            "action":"post",
            "task_id":task_id,
            "topic":"blockers",
            "gate":"contract",
            "payload":blocker_payload.clone(),
            "author":"release-coordinator"
        }),
    );
    let blocker_id = blocker_post["id"].as_u64().unwrap();
    let blocker_full = client.payload(
        "task_blackboard",
        json!({"action":"inspect","message_id":blocker_id}),
    );
    assert_eq!(blocker_full["message"]["payload"], blocker_payload);
    let embedded_context =
        client.payload("task_manage", json!({"action":"context","task_id":task_id}));
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

    let (_, context_response) =
        client.call("task_manage", json!({"action":"context","task_id":task_id}));
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
    assert!(context_markdown.contains(&format!("**Task ID**: `{cli_task_id}`")));
    assert!(context_markdown.contains(&format!("Task id: `{task_id}`")));
    assert!(context_markdown.contains("Gate: `contract`"));
    assert!(context_markdown.contains(task_id));
    assert!(context_markdown.contains("contract"));
    assert!(!context_markdown.contains(&format!("BLOCKER (#{})", decision_post["id"])));

    let list_args = json!({
        "status":"all",
        "milestone_ref":"docs/milestones/010-context.md"
    });
    let listed = client.payload("task_list", list_args.clone());
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
    let (_, list_response) = client.call("task_list", list_args);
    let list_markdown = list_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(list_markdown.contains(dependency_task_id));
    assert!(list_markdown.contains("Dependencies"));
    assert!(list_markdown.contains("routing-task-with-a-long-identity-for-blocker-regression"));

    let (_, claim_response) = client.call(
        "task_claim",
        json!({
            "task_id":decision_task_id,
            "stage":"contract",
            "worker_id":"markdown-author",
            "worktree":workspace.0
        }),
    );
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

    let (_, minimal_claim_response) = client.call(
        "task_claim",
        json!({
            "task_id":minimal_markdown_task_id,
            "stage":"contract",
            "worker_id":"minimal-markdown-author",
            "worktree":workspace.0,
            "bundle":false
        }),
    );
    let minimal_claim_markdown = minimal_claim_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(minimal_claim_markdown.contains("# Task claim"));
    assert!(minimal_claim_markdown.contains("Stage guidance"));
    assert!(minimal_claim_markdown.contains("Write scope"));
    assert!(!minimal_claim_markdown.contains("## Scope and proof context"));

    let cli_context = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "context",
            decision_task_id,
        ])
        .output()
        .unwrap();
    assert!(
        cli_context.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_context.stderr)
    );
    let cli_context_markdown = String::from_utf8(cli_context.stdout).unwrap();
    assert!(cli_context_markdown.starts_with("# Task context"));
    assert!(cli_context_markdown.contains("Stage guidance"));
    assert!(cli_context_markdown.contains("CAUTION] BLOCKER"));
    assert!(cli_context_markdown.contains("release-coordinator"));
}

#[test]
fn task_list_status_filter_is_typed_and_cli_flags_are_available() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones/archive")).unwrap();
    workspace.write("src/lib.rs", "pub fn task_list_fixture() {}\n");
    workspace.write(
        "docs/milestones/010-active.md",
        "---\nid: m-task-list\ntitle: Task list\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: visible\ntarget: Visible task\nproof_policy: direct-proof\nscope: [src/]\nsubtasks:\n  - subtask_ref: explain\n    title: Detailed subtask\n    status: completed\n    evidence: Verified through the listing seam\n```\n",
    );
    workspace.write(
        "docs/milestones/011-planned.md",
        "---\nid: m-planned-list\ntitle: Planned task list\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: planned\ntarget: Planned task\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    workspace.write(
        "docs/milestones/archive/009-completed.md",
        "---\nid: m-completed-list\ntitle: Completed task list\ndoc_type: contract\nstatus: completed\n---\n# Tasks\n```yaml\ntask_ref: completed\ntarget: Completed task\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"010-active"}),
    );
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let planned = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"011-planned"}),
    );
    let planned_task_id = planned["tasks"][0]["task_id"].as_str().unwrap();
    let completed = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"009-completed"}),
    );
    let completed_task_id = completed["tasks"][0]["task_id"].as_str().unwrap();

    let (_, catalog) = client.request("tools/list", json!({}));
    let task_list = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_list")
        .unwrap();
    let status_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["milestone_status"]).unwrap();
    for status in ["active", "planned", "completed", "all"] {
        assert!(status_schema.contains(status));
    }
    assert!(!status_schema.contains("cancelled"));
    let task_status_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["status"]).unwrap();
    for status in [
        "ready",
        "in_progress",
        "blocked",
        "parked",
        "completed",
        "cancelled",
        "all",
    ] {
        assert!(task_status_schema.contains(status));
    }
    assert!(task_list["inputSchema"]["properties"]
        .get("include_archived")
        .is_none());
    let detail_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["detail"]).unwrap();
    assert!(detail_schema.contains("compact"));
    assert!(detail_schema.contains("full"));

    let (_, invalid_status) = client.call("task_list", json!({"milestone_status":"cancelled"}));
    assert!(invalid_status.get("error").is_some() || invalid_status["result"]["isError"] == true);
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"active"}))["tasks"][0]["task_id"],
        task_id
    );
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"planned"}))["tasks"][0]["task_id"],
        planned_task_id
    );
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"completed"}))["tasks"][0]["task_id"],
        completed_task_id
    );
    let mut mcp_all: Vec<_> = client.payload("task_list", json!({"milestone_status":"all"}))
        ["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap().to_owned())
        .collect();
    mcp_all.sort();
    let mut expected_all = vec![
        task_id.to_owned(),
        planned_task_id.to_owned(),
        completed_task_id.to_owned(),
    ];
    expected_all.sort();
    assert_eq!(mcp_all, expected_all);

    let cli_filter = |flags: &[&str]| {
        let output = json_cli()
            .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
            .args(flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        payload["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["task_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(cli_filter(&["--milestone-status", "active"]), [task_id]);
    assert_eq!(
        cli_filter(&["--milestone-status", "planned"]),
        [planned_task_id]
    );
    assert_eq!(
        cli_filter(&["--milestone-status", "completed"]),
        [completed_task_id]
    );
    let mut cli_all = cli_filter(&["--milestone-status", "all"]);
    cli_all.sort();
    assert_eq!(cli_all, expected_all);
    assert_eq!(cli_filter(&["--planned"]), [planned_task_id]);
    assert_eq!(cli_filter(&["--completed"]), [completed_task_id]);
    let mut cli_shorthand_all = cli_filter(&["--all"]);
    cli_shorthand_all.sort();
    assert_eq!(cli_shorthand_all, expected_all);

    let cli_help = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--help",
        ])
        .output()
        .unwrap();
    assert!(cli_help.status.success());
    let cli_help = String::from_utf8(cli_help.stdout).unwrap();
    assert!(cli_help.contains("--all"));
    assert!(cli_help.contains("cancelled milestones pending sync pruning"));

    assert_eq!(cli_filter(&["--full"]), [task_id]);

    let compact = client.payload("task_list", json!({}));
    assert_eq!(compact["tasks"].as_array().unwrap().len(), 1);
    let compact_subtask = &compact["tasks"][0]["subtasks"][0];
    assert_eq!(compact_subtask["subtask_ref"], "explain");
    assert_eq!(compact_subtask["status"], "completed");
    assert!(compact_subtask.get("title").is_none());
    assert!(compact_subtask.get("evidence").is_none());

    let full = client.payload("task_list", json!({"detail":"full"}));
    assert_eq!(full["tasks"][0]["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        full["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let (_, full_markdown_response) = client.call("task_list", json!({"detail":"full"}));
    let full_markdown = full_markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(full_markdown.contains(task_id));
    assert!(full_markdown.contains("Detailed subtask"));
    assert!(full_markdown.contains("Verified through the listing seam"));
    assert!(full_markdown.contains("completed"));
    let subtask_details = client.payload(
        "task_manage",
        json!({"action":"subtask_list","task_id":task_id}),
    );
    assert_eq!(subtask_details["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        subtask_details["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":task_id}));
    assert_eq!(inspected["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        inspected["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );

    let cli_full = json_cli()
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone-status",
            "active",
            "--full",
        ])
        .output()
        .unwrap();
    assert!(
        cli_full.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_full.stderr)
    );
    let cli_full_json: Value = serde_json::from_slice(&cli_full.stdout).unwrap();
    assert_eq!(
        cli_full_json["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let cli_full_markdown = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone-status",
            "active",
            "--full",
        ])
        .output()
        .unwrap();
    assert!(
        cli_full_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_full_markdown.stderr)
    );
    let cli_full_markdown = String::from_utf8(cli_full_markdown.stdout).unwrap();
    assert!(cli_full_markdown.starts_with("# Task list"));
    assert!(cli_full_markdown.contains("Detailed subtask"));
    assert!(cli_full_markdown.contains("Verified through the listing seam"));

    for conflicting_flags in [
        vec!["--planned", "--completed"],
        vec!["--planned", "--all"],
        vec!["--completed", "--all"],
        vec!["--milestone-status", "active", "--planned"],
        vec!["--milestone-status", "all", "--all"],
    ] {
        let conflict = json_cli()
            .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
            .args(conflicting_flags)
            .output()
            .unwrap();
        assert!(!conflict.status.success());
    }
}

#[test]
fn task_context_candidate_snapshot_delivery_reject_and_completed_receipt() {
    use contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS;
    let workspace = Workspace::new();
    let reviewer = Workspace::new();
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(reviewer.0.join("src")).unwrap();
    fs::create_dir_all(reviewer.0.join("docs/milestones")).unwrap();

    let manifest = "---\nid: m-snapshots\ntitle: Snapshots and Rejections\ndoc_type: contract\nstatus: active\ninvariants: [review-contour-check]\n---\n```yaml\ntask_ref: candidate-task\ntarget: Deliver candidate\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    workspace.write("docs/milestones/010-snap.md", manifest);
    reviewer.write("docs/milestones/010-snap.md", manifest);
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: .forge/tasks.sqlite\n",
    );
    reviewer.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\ntasks_db: {}\n",
            workspace.0.join(".forge/tasks.sqlite").display()
        ),
    );

    let mut client = Client::new(&workspace);
    let sync_res = client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-snap.md"}),
    );
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    // 1. Pass contract proof without supplying a caller-selected commit.
    let contract_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "contract", "worker_id": "author", "worktree": workspace.0}),
    );
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "contract",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "contract",
                "claim_revision": contract_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "author",
                "worktree": workspace.0,
                "proof": {"contract_proof": {"seam_test_ref": "tests/test.rs", "red_exit_code": 0}}
            }
        }),
    );

    // 2. Pass build WITHOUT a commit in evidence
    let build_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.0}),
    );
    let first_build = client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build",
                "claim_revision": build_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "proof": {"command_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    let first_candidate = first_build["snapshot"]["commit"].as_str().unwrap();

    // The engine pins the scoped candidate even when build evidence omits a commit.
    let review_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    assert_eq!(
        review_claim["context_bundle"]["candidate_snapshot"]["commit"],
        first_candidate
    );

    // Reject review with findings -> task bounces back to build
    let contours: serde_json::Map<String, Value> = REVIEW_CONTOURS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                json!({"applicable": true, "evidence": "verified"}),
            )
        })
        .collect();
    let review_reject_proof =
        json!({"review_proof": {"decision": "reject", "contours": contours.clone()}});
    let review_findings = json!({
        "decision": "reject",
        "notes": "Need build commit",
        "blackboard": ["durable-review-evidence"],
        "blackboard_messages": ["durable-message-field-evidence"]
    });
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review",
            "action": "reject",
            "evidence": {
                "task_id": task_id,
                "stage": "review",
                "claim_revision": review_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "proof": review_reject_proof
            },
            "findings": review_findings
        }),
    );

    // Build claim after review reject: verify unresolved_review_findings contains review_findings
    let build_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.0}),
    );
    assert_eq!(
        build_claim2["context_bundle"]["unresolved_review_findings"],
        review_findings
    );

    let build_context = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
    assert_eq!(
        build_context["context_bundle"]["unresolved_review_findings"],
        review_findings
    );
    let (_, response) = client.call(
        "task_manage",
        json!({"action": "context", "task_id": task_id, "format": "markdown"}),
    );
    let build_context_markdown = response["result"]["content"][0]["text"]
        .as_str()
        .expect("Build context should return Markdown text");
    assert!(
        build_context_markdown.contains("**blackboard**")
            && build_context_markdown.contains("durable-review-evidence")
            && build_context_markdown.contains("**blackboard_messages**")
            && build_context_markdown.contains("durable-message-field-evidence"),
        "Build context should render all review-finding fields: {build_context_markdown}"
    );

    let second_build = client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build",
                "claim_revision": build_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "proof": {"command_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    let build_commit = second_build["snapshot"]["commit"].as_str().unwrap();

    // Rebuilding the unchanged scoped tree keeps the pinned candidate.
    let review_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    assert_eq!(
        review_claim2["context_bundle"]["candidate_snapshot"]["commit"],
        build_commit
    );

    // Pass review
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "review",
                "claim_revision": review_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "proof": {"review_proof": {"decision": "pass", "contours": contours.clone()}}
            }
        }),
    );

    // 4. Claim deliver and REJECT delivery with findings
    let deliver_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "deliver", "worker_id": "delivery-lead", "worktree": workspace.0}),
    );
    // At deliver before completion, receipt must be None
    assert!(deliver_claim["context_bundle"].get("receipt").is_none());

    let delivery_findings = json!({"decision": "reject", "notes": "Missing changelog entry"});
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "deliver",
            "action": "reject",
            "evidence": {
                "task_id": task_id,
                "stage": "deliver",
                "claim_revision": deliver_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "delivery-lead",
                "worktree": workspace.0,
                "proof": {"delivery_proof": {"status": "rejected"}}
            },
            "findings": delivery_findings
        }),
    );

    // Task bounced back to build: claim build and verify unresolved_review_findings contains delivery findings
    let rebounce_build_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build", "worker_id": "builder", "worktree": workspace.0}),
    );
    assert_eq!(
        rebounce_build_claim["context_bundle"]["unresolved_review_findings"],
        delivery_findings
    );

    // 5. Complete build, review, deliver and verify receipt in context_bundle
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build",
                "claim_revision": rebounce_build_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "proof": {"command_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    let review_claim3 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "review",
                "claim_revision": review_claim3["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "proof": {"review_proof": {"decision": "pass", "contours": contours}}
            }
        }),
    );

    let deliver_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "deliver", "worker_id": "delivery-lead", "worktree": workspace.0}),
    );
    assert!(deliver_claim2["context_bundle"].get("receipt").is_none());

    let delivered = client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "deliver",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "deliver",
                "claim_revision": deliver_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "delivery-lead",
                "worktree": workspace.0,
                "proof": {"delivery_proof": {"status": "passed"}}
            }
        }),
    );
    assert_eq!(
        delivered["snapshot"]["commit"],
        delivered["receipt"]["commit"]
    );

    // Completed task: context_bundle contains receipt, root envelope does not
    let completed_context = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
    assert!(
        completed_context.get("receipt").is_none(),
        "root envelope must prune receipt"
    );
    assert!(
        completed_context["context_bundle"]["receipt"].is_object(),
        "context_bundle must contain receipt for completed task"
    );
    assert_eq!(
        completed_context["context_bundle"]["receipt"]["commit"],
        delivered["receipt"]["commit"]
    );
    assert_eq!(
        completed_context["context_bundle"]["latest_snapshot"]["commit"],
        build_commit
    );
    let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":task_id}));
    let findings = inspected["findings"].as_array().unwrap();
    assert!(findings.len() >= 2);
    let review_finding = findings
        .iter()
        .find(|record| {
            record["findings"]
                .as_str()
                .is_some_and(|text| text.contains("durable-review-evidence"))
        })
        .unwrap();
    let finding_text = review_finding["findings"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(finding_text).unwrap(),
        review_findings
    );
    assert_eq!(inspected["latest_snapshot"]["commit"], build_commit);
    assert_eq!(
        inspected["receipt"]["commit"],
        delivered["receipt"]["commit"]
    );
    let (_, inspect_response) =
        client.call("task_manage", json!({"action":"inspect","task_id":task_id}));
    let inspect_markdown = inspect_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
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
        assert!(
            inspect_markdown.contains(expected),
            "missing {expected:?} in task inspect Markdown:\n{inspect_markdown}"
        );
    }
}

#[test]
fn wave_sync_and_atomic_next_preserve_parked_worktrees() {
    use super::support::{evidence, tasks, TasksStore};
    use std::sync::Barrier;

    let mut failures = Vec::new();
    let mut check = |condition: bool, requirement: &str| {
        if !condition {
            failures.push(requirement.to_owned());
        }
    };
    let call = |client: &mut Client, tool: &str, mut args: Value| {
        args["format"] = json!("json");
        let response = client.call(tool, args).1;
        response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(response)
    };
    let fixture = |name: &str, specs: &[(&str, &str)]| {
        let workspace = Workspace::new();
        workspace.write(
            "forge-mcp.yaml",
            "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
        );
        let mut manifest = format!(
            "---\nid: m-{name}\ntitle: Dispatch\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n"
        );
        for (reference, scope) in specs {
            manifest.push_str(&format!(
                "```yaml\ntask_ref: {reference}\ntarget: Deliver {reference}\nproof_policy: seam-test-first\nscope: [{scope}]\nscope_roots: [src/]\n```\n"
            ));
            let file = if scope.ends_with('/') {
                format!("{scope}lib.rs")
            } else {
                (*scope).to_owned()
            };
            workspace.write(&file, "pub fn seam() {}\n");
        }
        workspace.write("docs/milestones/010-dispatch.md", &manifest);
        let mut client = Client::new(&workspace);
        let synced = client.payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"docs/milestones/010-dispatch.md"}),
        );
        let ids: Vec<String> = specs
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
    };
    let advance =
        |workspace: &Workspace, client: &mut Client, id: &str, stage: &str, worker: &str| {
            client.payload(
                "task_claim",
                json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":workspace.0}),
            );
            let store = TasksStore::open(&workspace.0.join("tasks.sqlite")).unwrap();
            let proof = evidence(&store.inspect(id).unwrap());
            client.payload(
                "task_submit",
                json!({"task_id":id,"stage":stage,"action":"pass","evidence":proof}),
            )
        };

    // Declaration order, rather than lexical identity, determines overlap edges.
    let (waves, mut client, ids) = fixture(
        "waves",
        &[
            ("z-first", "src/shared/"),
            ("b-second", "src/shared/lib.rs"),
            ("a-third", "src/shared/lib.rs"),
            ("a-independent", "src/other.rs"),
        ],
    );
    let store = TasksStore::open(&waves.0.join("tasks.sqlite")).unwrap();
    let originals: Vec<_> = ids.iter().map(|id| store.inspect(id).unwrap()).collect();
    let mut statement = store
        .connection
        .prepare(
            "SELECT task_id, dependency_id FROM task_dependencies ORDER BY task_id, dependency_id",
        )
        .unwrap();
    let edges: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut expected = vec![
        (ids[1].clone(), ids[0].clone()),
        (ids[2].clone(), ids[0].clone()),
        (ids[2].clone(), ids[1].clone()),
    ];
    expected.sort();
    check(
        edges == expected,
        "sync must persist only top-down overlap dependencies",
    );
    for (id, wave) in ids.iter().zip([0, 1, 2, 0]) {
        let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
        check(
            inspected["wave_index"] == wave,
            "sync must expose longest-dependency wave indexes",
        );
    }
    client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-dispatch.md"}),
    );
    for original in &originals {
        let current = store.inspect(&original.task_id).unwrap();
        check(
            current.spec.depends_on.is_empty(),
            "derived dependencies must not rewrite TaskSpec.depends_on",
        );
        check(
            current.digest == original.digest,
            "repeat sync must preserve authority digests",
        );
    }
    let first = call(
        &mut client,
        "task_manage",
        json!({"action":"next","worker_id":"wave-worker","worktree":waves.0}),
    );
    check(
        first["task_id"] == ids[3],
        "equal-wave contract dispatch must sort by task_id",
    );
    let second = call(
        &mut client,
        "task_manage",
        json!({"action":"next","worker_id":"wave-worker-2","worktree":waves.0}),
    );
    check(
        second["task_id"] == ids[0],
        "inferred dependencies must permit the first overlapping owner to claim",
    );
    check(
        second["context_bundle"].is_object(),
        "next must return the claimed zero-shot context bundle",
    );

    // Independent deliver/review gates outrank contract work, while the builder
    // remains ineligible to review or deliver its own accepted candidate.
    let (priority, mut client, ids) = fixture(
        "priority",
        &[
            ("z-deliver", "src/deliver.rs"),
            ("b-review", "src/review.rs"),
            ("a-contract", "src/contract.rs"),
        ],
    );
    for id in &ids[..2] {
        advance(&priority, &mut client, id, "contract", "builder");
        advance(&priority, &mut client, id, "build", "builder");
    }
    advance(&priority, &mut client, &ids[0], "review", "reviewer");
    let excluded = call(
        &mut client,
        "task_manage",
        json!({"action":"next","worker_id":"builder","worktree":priority.0}),
    );
    check(
        excluded["task_id"] == ids[2],
        "next must skip the builder's own review and deliver gates",
    );
    for (id, stage) in [(&ids[0], "deliver"), (&ids[1], "review")] {
        let selected = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"independent","worktree":priority.0}),
        );
        check(
            selected["task_id"] == *id && selected["stage"] == stage,
            "next must prioritize independent deliver, then review",
        );
    }

    let (parking, mut client, ids) = fixture(
        "parking",
        &[
            ("holder", "src/holder.rs"),
            ("parked", "src/parked.rs"),
            ("ordinary", "src/ordinary.rs"),
        ],
    );
    parking.write(
        "src/parked.rs",
        "pub fn unfinished_work_is_preserved() {}\n",
    );
    let unfinished = fs::read(parking.0.join("src/parked.rs")).unwrap();
    for (id, worker) in [(&ids[0], "holder-worker"), (&ids[1], "park-worker")] {
        advance(&parking, &mut client, id, "contract", worker);
        client.payload(
            "task_claim",
            json!({"task_id":id,"stage":"build","worker_id":worker,"worktree":parking.0}),
        );
    }
    let parked = call(
        &mut client,
        "task_manage",
        json!({"action":"extend_scope","task_id":ids[1],"paths":["src/holder.rs"]}),
    );
    check(
        parked["status"] == "parked" && parked["blocked_by"] == ids[0],
        "extend_scope must park behind the active holder",
    );
    if parked["status"] == "parked" {
        let task_ids = |payload: &Value| {
            let mut listed: Vec<String> = payload["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|task| task["task_id"].as_str().unwrap().to_owned())
                .collect();
            listed.sort();
            listed
        };
        let mut all_ids = ids.clone();
        all_ids.sort();
        for (status, expected) in [
            ("parked", vec![ids[1].clone()]),
            ("ready", vec![ids[2].clone()]),
            ("in_progress", vec![ids[0].clone()]),
            ("blocked", Vec::new()),
            ("completed", Vec::new()),
            ("all", all_ids),
        ] {
            let listed = call(&mut client, "task_list", json!({"status":status}));
            check(
                task_ids(&listed) == expected,
                &format!(
                    "MCP status={status} must preserve the exact task selection after parking"
                ),
            );
            let output = json_cli()
                .args([
                    "--root",
                    parking.0.to_str().unwrap(),
                    "task",
                    "list",
                    "--status",
                    status,
                ])
                .output()
                .unwrap();
            let listed = serde_json::from_slice::<Value>(&output.stdout).unwrap_or(Value::Null);
            check(
                output.status.success() && task_ids(&listed) == expected,
                &format!(
                    "CLI --status {status} must preserve the exact task selection after parking"
                ),
            );
        }
        check(
            task_ids(&call(&mut client, "task_list", json!({}))) == vec![ids[2].clone()],
            "default MCP status must remain ready and exclude parked work",
        );
        let default_cli = json_cli()
            .args(["--root", parking.0.to_str().unwrap(), "task", "list"])
            .output()
            .unwrap();
        let listed = serde_json::from_slice::<Value>(&default_cli.stdout).unwrap_or(Value::Null);
        check(
            default_cli.status.success() && task_ids(&listed) == vec![ids[2].clone()],
            "default CLI status must remain ready and exclude parked work",
        );
        let store = TasksStore::open(&parking.0.join("tasks.sqlite")).unwrap();
        let descriptor = serde_json::to_value(store.inspect(&ids[1]).unwrap()).unwrap();
        check(
            descriptor["parked_worker_id"] == "park-worker",
            "parking must retain the builder identity",
        );
        check(
            descriptor["parked_worktree"] == parking.0.to_str().unwrap(),
            "parking must retain the original worktree",
        );
        check(
            descriptor["spec"]["scope"]
                .as_array()
                .unwrap()
                .contains(&json!("src/holder.rs")),
            "parking must admit the requested scope",
        );
        let open: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM task_claims WHERE task_id=?1 AND ended=0",
                [&ids[1]],
                |row| row.get(0),
            )
            .unwrap();
        check(
            open == 0,
            "parking must release its active claim atomically",
        );
        let blocked: i64 = store.connection.query_row("SELECT COUNT(*) FROM task_dependencies WHERE task_id=?1 AND dependency_id=?2 AND satisfied=0", [&ids[1], &ids[0]], |row| row.get(0)).unwrap();
        check(
            blocked == 1,
            "parking must persist its unsatisfied holder dependency",
        );
        let notes: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM task_blackboard WHERE task_id=?1 AND topic='blockers'",
                [&ids[1]],
                |row| row.get(0),
            )
            .unwrap();
        check(notes > 0, "parking must publish a blocker blackboard note");
        let foreign = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"foreign","worktree":parking.0}),
        );
        check(
            foreign["task_id"] == ids[2],
            "another worker must not steal blocked parked work",
        );
        let active = store.inspect(&ids[0]).unwrap();
        client.payload(
            "task_submit",
            json!({"task_id":ids[0],"stage":"build","action":"pass","evidence":evidence(&active)}),
        );
        advance(&parking, &mut client, &ids[0], "review", "holder-reviewer");
        advance(&parking, &mut client, &ids[0], "deliver", "holder-reviewer");
        let ordinary = store.inspect(&ids[2]).unwrap();
        client.payload("task_submit", json!({"task_id":ids[2],"stage":"contract","action":"pass","evidence":evidence(&ordinary)}));
        let another_worktree = Workspace::new();
        another_worktree.write(
            "forge-mcp.yaml",
            &format!(
                "roots: []\ndocs: []\ntasks_db: {}\n",
                parking.0.join("tasks.sqlite").display()
            ),
        );
        another_worktree.write(
            "docs/milestones/010-dispatch.md",
            &fs::read_to_string(parking.0.join("docs/milestones/010-dispatch.md")).unwrap(),
        );
        let resumed = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"park-worker","worktree":another_worktree.0}),
        );
        check(
            resumed["task_id"] == ids[1] && resumed["stage"] == "build",
            "same-worker parked work must outrank ordinary build work after unblocking",
        );
        check(
            resumed["worktree"] == parking.0.to_str().unwrap(),
            "resume must use the preserved worktree rather than the dispatch argument",
        );
        check(
            fs::read(parking.0.join("src/parked.rs")).unwrap() == unfinished,
            "parking and resume must preserve unfinished source bytes",
        );
    }

    // Two real MCP server processes compete for one durable claim.
    let (race, _client, ids) = fixture("race", &[("only", "src/only.rs")]);
    let barrier = Barrier::new(2);
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = ["racer-a", "racer-b"]
            .into_iter()
            .map(|worker| {
                let workspace = &race;
                let barrier = &barrier;
                let call = &call;
                scope.spawn(move || {
                    let mut client = Client::new(workspace);
                    barrier.wait();
                    call(
                        &mut client,
                        "task_manage",
                        json!({"action":"next","worker_id":worker,"worktree":workspace.0}),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    check(
        outcomes
            .iter()
            .filter(|result| result["task_id"] == ids[0])
            .count()
            == 1,
        "concurrent next requests must return exactly one successful claim",
    );
    let store = TasksStore::open(&race.0.join("tasks.sqlite")).unwrap();
    let open: i64 = store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_claims WHERE task_id=?1 AND ended=0",
            [&ids[0]],
            |row| row.get(0),
        )
        .unwrap();
    check(
        open == 1,
        "concurrent next must persist exactly one open claim",
    );

    let (cli, _client, ids) = fixture("cli-next", &[("only", "src/only.rs")]);
    let output = json_cli()
        .args([
            "--root",
            cli.0.to_str().unwrap(),
            "task",
            "next",
            "--worker",
            "cli-worker",
            "--worktree",
            cli.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let selected: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    check(
        output.status.success()
            && selected["task_id"] == ids[0]
            && selected["context_bundle"].is_object(),
        "CLI task next must expose the same atomic claim and context seam",
    );

    // The engine owns diagnostic text; MCP must carry it verbatim.
    let (errors, mut client, ids) = fixture("diagnostics", &[("only", "src/only.rs")]);
    client.payload(
        "task_claim",
        json!({"task_id":ids[0],"stage":"contract","worker_id":"owner","worktree":errors.0}),
    );
    let args = json!({"task_id":ids[0],"stage":"contract","worker_id":"other","worktree":errors.0});
    let engine_error = tasks::claim(&errors.0, serde_json::from_value(args.clone()).unwrap())
        .unwrap_err()
        .to_string();
    let propagated = call(&mut client, "task_claim", args);
    check(
        propagated["error"]["message"] == engine_error,
        "MCP must propagate the engine lifecycle diagnostic verbatim",
    );
    check(
        engine_error.starts_with(&format!("TASK_ALREADY_CLAIMED: {}. ", ids[0]))
            && engine_error.contains(". Do: ")
            && !engine_error.contains('\n'),
        "lifecycle diagnostics must use CODE: task_id. facts. Do: action on one line",
    );

    assert!(
        failures.is_empty(),
        "wave/dispatch contract failures:\n{}",
        failures.join("\n")
    );
}
