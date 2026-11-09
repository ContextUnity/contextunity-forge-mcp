use crate::{
    common::cli::{run_cli, run_cli_json},
    harness::McpTestHarness,
};
use clap::Parser;
use contextunity_forge_mcp::{
    cli::{guide, Cli},
    core::response::MAX_OUTPUT_BYTES,
};
use serde_json::{json, Value};

const GUIDANCE_TOPICS: [&str; 6] = ["lifecycle", "acdd", "gates", "subtasks", "code", "docs"];

#[test]
fn guide_unknown_topic_preserves_diagnostics_across_surfaces() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    let mut harness = harness.unwrap();
    assert!(guide::run(workspace.root(), "unknown-topic", false)
        .unwrap_err()
        .to_string()
        .contains("unknown guide topic"));
    let mcp = harness
        .call("forge_guide", json!({"topic":"unknown-topic"}))
        .unwrap()
        .1;
    assert_eq!(mcp["result"]["isError"], true);
    assert!(mcp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown guide topic"));
    let cli = run_cli_json(&workspace, &["guide", "unknown-topic"]);
    assert!(!cli.status.success());
    assert!(String::from_utf8_lossy(&cli.stderr).contains("unknown guide topic"));
    let query = guide::run(workspace.root(), "query", false).unwrap();
    assert_eq!(guide::run(workspace.root(), " ", false).unwrap(), query);
    assert_eq!(
        serde_json::from_slice::<Value>(&run_cli_json(&workspace, &["guide", " "]).stdout).unwrap(),
        query
    );
    harness.finish().unwrap();
}

#[test]
fn guide_init_preserves_force_and_bypasses_invalid_adapter() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    let mut harness = harness.unwrap();
    workspace.write("forge-mcp.yaml", "roots: [\n");
    assert!(guide::run(workspace.root(), "init", false).is_err());
    assert_eq!(
        harness
            .call("forge_guide", json!({"topic":"init"}))
            .unwrap()
            .1["result"]["isError"],
        true
    );
    assert!(!run_cli_json(&workspace, &["guide", "init"])
        .status
        .success());
    assert_eq!(
        std::fs::read_to_string(workspace.path("forge-mcp.yaml")).unwrap(),
        "roots: [\n"
    );
    let response = harness
        .call("forge_guide", json!({"topic":"init","force":true}))
        .unwrap()
        .1;
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        std::fs::read_to_string(workspace.path("forge-mcp.yaml")).unwrap(),
        guide::TEMPLATE
    );
    workspace.write("forge-mcp.yaml", "roots: [\n");
    let cli = run_cli_json(&workspace, &["guide", "init", "--force"]);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path("forge-mcp.yaml")).unwrap(),
        guide::TEMPLATE
    );
    harness.finish().unwrap();
}

#[test]
fn guide_cli_capabilities_follow_explicit_configuration() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    harness.unwrap().finish().unwrap();
    workspace.write(
        "configs/guide.yaml",
        "modules:\n  codemap: false\n  docs: false\n  acdd: true\n  blackboard: false\n",
    );
    let output = run_cli_json(
        &workspace,
        &["--config", "configs/guide.yaml", "guide", "query"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let query: Value = serde_json::from_slice(&output.stdout).unwrap();
    let topics = query["topics"].as_array().unwrap();
    assert!(!topics.contains(&json!("code")));
    assert!(!topics.contains(&json!("docs")));
    assert!(topics.contains(&json!("lifecycle")));
    let output = run_cli_json(
        &workspace,
        &["--config", "configs/guide.yaml", "guide", "lifecycle"],
    );
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("task_blackboard"));
    let default = guide::run(workspace.root(), "query", false).unwrap();
    assert!(default["topics"]
        .as_array()
        .unwrap()
        .contains(&json!("code")));
}

#[test]
fn lifecycle_guide_preserves_live_claim_and_proof_authority() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    let mut harness = harness.expect("guide workspace");
    let shared = guide::run(workspace.root(), "lifecycle", false);
    eprintln!("production lifecycle guide: {shared:?}");
    let mcp = harness
        .call("forge_guide", json!({"topic":"lifecycle", "format":"json"}))
        .expect("MCP lifecycle request")
        .1;
    let cli = run_cli_json(&workspace, &["guide", "lifecycle"]);
    eprintln!("MCP lifecycle response: {mcp}");
    assert!(mcp.get("error").is_none());
    assert!(mcp["result"]["isError"] != true);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(shared.is_ok());
    let shared = shared.expect("supported lifecycle topic");
    let mcp: Value =
        serde_json::from_str(mcp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(mcp, shared);
    assert_eq!(serde_json::from_slice::<Value>(&cli.stdout).unwrap(), mcp);
    let text = mcp.to_string().to_lowercase();
    for operation in [
        "task_list",
        "task_claim",
        "task_manage",
        "verify",
        "task_submit",
        "task run",
        "milestone run",
        "task_blackboard",
    ] {
        assert!(
            text.contains(operation),
            "missing lifecycle operation {operation}"
        );
    }
    let authority = mcp["authority"].to_string().to_lowercase();
    for source in ["profile", "workflow_guidance", "example"] {
        assert!(
            authority.contains(source),
            "missing profile authority distinction {source}"
        );
    }
    let runners = mcp["runners"].to_string().to_lowercase();
    for rule in [
        "runner",
        "independent",
        "resume",
        "findings",
        "fingerprint",
        "submission",
    ] {
        assert!(
            runners.contains(rule),
            "missing live runner boundary {rule}"
        );
    }
    for code in [
        "TASK_ALREADY_CLAIMED",
        "TASK_STALE_CLAIM",
        "TASK_AUTO_PROOF_UNAVAILABLE",
        "TASK_CANDIDATE_MISMATCH",
    ] {
        assert!(
            mcp["recovery"][code]
                .as_str()
                .is_some_and(|next| !next.trim().is_empty()),
            "missing next action for {code}"
        );
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn acdd_guide_distinguishes_execution_subjects_and_planned_logging() {
    let mut harness = McpTestHarness::indexed(&[]).expect("guide workspace");
    let value = harness
        .payload("forge_guide", json!({"topic":"acdd"}))
        .expect("ACDD guide");
    for (subject, terms) in [
        (
            "workflow",
            &[
                "root",
                "taskless",
                "save: none",
                "workflow run",
                "planned",
                "log",
            ][..],
        ),
        ("plan", &["active", "snapshot", "no auto", "plan run"][..]),
        (
            "milestone",
            &[
                "synthesis",
                "wave",
                "verif",
                "independent",
                "handoff",
                "milestone run",
            ][..],
        ),
    ] {
        let text = value["subjects"][subject].to_string().to_lowercase();
        for term in terms {
            assert!(text.contains(term), "{subject} must explain {term}: {text}");
        }
    }
    assert!(value["gates"].is_array());
    assert!(value["proof_policies"].is_object());
    harness.finish().expect("guide cleanup");
}

#[test]
fn gates_guide_matches_conditions_routes_and_snapshot_semantics() {
    let mut harness = McpTestHarness::indexed(&[]).expect("guide workspace");
    let value = harness.payload("forge_guide", json!({"topic":"gates"}));
    assert!(value.is_ok());
    let value = value.unwrap();
    for (condition, terms) in [
        ("code", &["scope", "non-doc"][..]),
        ("doc", &["scope", "all", "nonempty"][..]),
        ("db_migration", &["sql", "migration"][..]),
        ("seam", &["concurrency", "speculative", "parallel"][..]),
        ("public_api", &["export", "callable"][..]),
    ] {
        let text = value["when"][condition].to_string().to_lowercase();
        for term in terms {
            assert!(
                text.contains(term),
                "{condition} condition must explain {term}"
            );
        }
    }
    let transitions = value["transitions"].to_string().to_lowercase();
    for term in ["pass", "fail", "loop", "finding", "profile"] {
        assert!(transitions.contains(term), "missing routing rule {term}");
    }
    let persistence = value["persistence"].to_string().to_lowercase();
    for term in [
        "pre",
        "post",
        "parentless",
        "snapshot",
        "branch",
        "commit",
        "configured",
        "immutable",
        "task_candidate_mismatch",
    ] {
        assert!(
            persistence.contains(term),
            "missing persistence boundary {term}"
        );
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn gates_guide_requires_observed_claim_bound_proof() {
    let mut harness = McpTestHarness::indexed(&[]).expect("guide workspace");
    let value = harness.payload("forge_guide", json!({"topic":"gates"}));
    assert!(value.is_ok());
    let text = value.unwrap()["verification"].to_string().to_lowercase();
    for term in [
        "verify",
        "without advancing",
        "submit",
        "claim-bound",
        "gate_execution_traces",
        "commands.output.tests",
        "seam_test_refs",
        "individual",
        "caller",
        "json",
        "not",
    ] {
        assert!(
            text.contains(term),
            "missing observed-proof rule {term}: {text}"
        );
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn subtasks_guide_matches_delivered_guard_and_evidence_boundary() {
    let mut harness = McpTestHarness::indexed(&[]).expect("guide workspace");
    let value = harness.payload("forge_guide", json!({"topic":"subtasks"}));
    assert!(value.is_ok());
    let value = value.unwrap();
    let text = value.to_string().to_lowercase();
    for term in [
        "subtask_add",
        "subtask_update",
        "subtask_list",
        "ready",
        "in_progress",
        "completed",
        "cancelled",
        "blocked",
        "task_subtasks_incomplete",
        "task_subtask_evidence_missing",
    ] {
        assert!(text.contains(term), "missing subtask guidance {term}");
    }
    assert_eq!(
        value["delivery"]["terminal_statuses"],
        json!(["completed", "cancelled"])
    );
    assert_eq!(
        value["delivery"]["evidence_required_for"],
        json!(["completed"])
    );
    let evidence = value["delivery"]["evidence_boundary"]
        .as_str()
        .expect("evidence explanation")
        .to_lowercase();
    for term in ["string", "not", "verif"] {
        assert!(
            evidence.contains(term),
            "subtask_update evidence is stored text, not independent proof"
        );
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn navigation_guides_retain_keys_and_execute_supported_examples() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[
        ("src/lib.rs", "pub fn guide_contract() {}\n"),
        ("docs/guide.md", "---\ntitle: Guide contract\ndoc_type: guide\n---\n# Guide contract\nNavigation example.\n"),
    ]);
    let mut harness = harness.expect("indexed guide workspace");
    for (topic, operations) in [
        (
            "code",
            &[
                "code_map_overview",
                "code_map_search",
                "code_map_inspect",
                "forge_read",
                "code_map_explain",
                "code_map_impact",
                "code_map_tests",
                "ast_grep_search",
                "candidate",
                "lexical",
                "verified",
            ][..],
        ),
        (
            "docs",
            &[
                "search_docs",
                "get_doc",
                "forge_read",
                "docs search",
                "docs get",
            ][..],
        ),
    ] {
        let value = harness.payload("forge_guide", json!({"topic":topic}));
        assert!(value.is_ok());
        let value = value.unwrap();
        let text = value.to_string();
        for operation in operations {
            assert!(text.contains(operation), "{topic} must cover {operation}");
        }
        if topic == "docs" {
            assert!(value["frontmatter"].is_string());
            assert!(value["invariant"].is_string());
            assert!(value["types"].is_array());
        }
        let example = &value["example"];
        let name = example["mcp"]["name"].as_str().expect("MCP example name");
        assert!(
            matches!(
                name,
                "code_map_overview" | "code_map_search" | "search_docs" | "get_doc" | "forge_read"
            ),
            "navigation example must use a read operation"
        );
        let observed = harness
            .call(name, example["mcp"]["arguments"].clone())
            .expect("execute MCP example")
            .1;
        assert!(observed.get("error").is_none(), "{observed}");
        assert_ne!(observed["result"]["isError"], true, "{observed}");
        let words = example["cli"]
            .as_str()
            .expect("CLI example")
            .split_whitespace()
            .collect::<Vec<_>>();
        assert_eq!(words.first().copied(), Some("contextunity-forge-mcp"));
        let output = run_cli_json(&workspace, &words[1..]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(serde_json::from_slice::<Value>(&output.stdout).is_ok());
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn guide_formats_share_semantics_and_markdown_within_budget() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    let mut harness = harness.expect("guide workspace");
    for topic in GUIDANCE_TOPICS.into_iter().chain(["query"]) {
        let mut markdown = None;
        for format in [None, Some("markdown"), Some("json")] {
            let mut args = json!({"topic":topic});
            let mut cli_args = vec!["guide", topic];
            if let Some(format) = format {
                args["format"] = json!(format);
                cli_args.extend(["--format", format]);
            }
            let (bytes, envelope) = harness.call("forge_guide", args).expect("guide request");
            assert!(bytes <= MAX_OUTPUT_BYTES);
            assert_ne!(envelope["result"]["isError"], true, "{topic}: {envelope}");
            let mcp = envelope["result"]["content"][0]["text"]
                .as_str()
                .expect("guide text");
            let cli = run_cli(&workspace, &cli_args);
            assert!(
                cli.status.success(),
                "{}",
                String::from_utf8_lossy(&cli.stderr)
            );
            let cli = String::from_utf8(cli.stdout).unwrap();
            if format == Some("json") {
                let mcp: Value = serde_json::from_str(mcp).expect("structured JSON");
                assert_eq!(serde_json::from_str::<Value>(&cli).unwrap(), mcp);
                if topic != "query" {
                    assert!(mcp["purpose"]
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty()));
                    assert!(mcp["steps"].as_array().is_some_and(|s| !s.is_empty()));
                    assert!(mcp["references"].as_array().is_some_and(|s| !s.is_empty()));
                }
            } else {
                assert_eq!(cli.trim_end_matches('\n'), mcp.trim_end_matches('\n'));
                assert!(mcp.starts_with('#'), "readable Markdown, not JSON");
                if let Some(default) = &markdown {
                    assert_eq!(default, mcp);
                } else {
                    markdown = Some(mcp.to_owned());
                }
            }
        }
    }
    harness.finish().expect("guide cleanup");
}

#[test]
fn guide_topic_index_and_capabilities_match_both_surfaces() {
    let (workspace, harness) = McpTestHarness::indexed_workspace(&[]);
    harness
        .expect("initial guide workspace")
        .finish()
        .expect("guide cleanup");
    for bits in 0u8..16 {
        let codemap = bits & 1 != 0;
        let docs = bits & 2 != 0;
        let acdd = bits & 4 != 0;
        let blackboard = bits & 8 != 0;
        workspace.write("forge-mcp.yaml", &format!("roots: [src/]\nmodules:\n  codemap: {codemap}\n  docs: {docs}\n  acdd: {acdd}\n  blackboard: {blackboard}\n"));
        let mut harness = McpTestHarness::new(&workspace).expect("module configuration");
        let query = harness
            .payload("forge_guide", json!({}))
            .expect("default query");
        assert_eq!(
            query,
            harness
                .payload("forge_guide", json!({"topic":"query"}))
                .unwrap()
        );
        assert_eq!(
            query,
            harness
                .payload("forge_guide", json!({"topic":" "}))
                .unwrap()
        );
        let cli = run_cli_json(&workspace, &["guide", "query"]);
        assert!(cli.status.success());
        assert_eq!(serde_json::from_slice::<Value>(&cli.stdout).unwrap(), query);
        let topics = query["topics"].as_array();
        assert!(topics.is_some());
        let topics = topics.unwrap();
        for (topic, enabled) in [
            ("query", true),
            ("init", true),
            ("adapter", true),
            ("validate", true),
            ("code", codemap),
            ("ast", codemap),
            ("docs", docs),
            ("lifecycle", acdd),
            ("acdd", acdd),
            ("gates", acdd),
            ("subtasks", acdd),
        ] {
            assert_eq!(topics.contains(&json!(topic)), enabled, "{bits}: {topic}");
            if matches!(topic, "init" | "adapter" | "validate" | "query") {
                continue;
            }
            let value = harness
                .payload("forge_guide", json!({"topic":topic}))
                .expect("known topic");
            let cli = run_cli_json(&workspace, &["guide", topic]);
            assert!(cli.status.success());
            assert_eq!(serde_json::from_slice::<Value>(&cli.stdout).unwrap(), value);
            assert_eq!(value == json!({}), !enabled, "{bits}: {topic}");
            if topic == "acdd" && acdd {
                for key in [
                    "blackboard_topics",
                    "blackboard_gate_targeting",
                    "blackboard_schema_version",
                ] {
                    assert_eq!(value.get(key).is_some(), blackboard, "{bits}: {key}");
                }
            }
            if topic == "lifecycle" && acdd {
                assert_eq!(value.to_string().contains("task_blackboard"), blackboard);
            }
        }
        for key in [
            "start",
            "sql_schema",
            "symbols",
            "snippets",
            "tests",
            "queries",
            "diagnostics",
            "recovery",
        ] {
            assert_eq!(query.get(key).is_some(), codemap, "{bits}: {key}");
        }
        assert_eq!(query.get("documents").is_some(), docs);
        assert_eq!(query.get("pages").is_some(), codemap || docs);
        harness.finish().expect("guide cleanup");
    }
}

#[test]
fn guide_discovery_and_examples_match_real_tools_and_commands() {
    let mut harness = McpTestHarness::indexed(&[]).expect("guide workspace");
    let list = harness
        .request_value("tools/list", json!({}))
        .expect("tool discovery");
    let tools = list["result"]["tools"].as_array().expect("tools array");
    let schema = tools
        .iter()
        .find(|tool| tool["name"] == "forge_guide")
        .expect("guide tool");
    let description = schema["inputSchema"]["properties"]["topic"]["description"]
        .as_str()
        .expect("topic discovery");
    for topic in GUIDANCE_TOPICS {
        assert!(description.contains(topic), "schema must advertise {topic}");
        let value = harness
            .payload("forge_guide", json!({"topic":topic}))
            .expect("supported guide");
        let name = value["example"]["mcp"]["name"]
            .as_str()
            .expect("MCP example");
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("real MCP tool");
        let args = value["example"]["mcp"]["arguments"]
            .as_object()
            .expect("example arguments");
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .expect("tool arguments");
        for key in args.keys() {
            assert!(properties.contains_key(key), "{name} has no {key} argument");
        }
        if let Some(required) = tool["inputSchema"]["required"].as_array() {
            for key in required {
                assert!(
                    args.contains_key(key.as_str().unwrap()),
                    "{name} example misses {key}"
                );
            }
        }
        let command = value["example"]["cli"].as_str().expect("CLI example");
        assert!(
            Cli::try_parse_from(command.split_whitespace()).is_ok(),
            "invalid command example: {command}"
        );
    }
    for args in [
        vec!["task", "run", "TASK_ID", "--in-place"],
        vec![
            "milestone",
            "run",
            "AC-003",
            "--in-place",
            "--target-base",
            "main",
        ],
        vec!["workflow", "run", "WORKFLOW"],
        vec!["plan", "run", "PLAN", "--in-place"],
        vec!["task", "verify", "TASK_ID", "--test", "tests/seam.rs::case"],
        vec![
            "task",
            "subtask",
            "update",
            "TASK_ID",
            "slice",
            "--status",
            "completed",
            "--evidence",
            "observed-command",
        ],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("contextunity-forge-mcp").chain(args)).is_ok());
    }
    harness.finish().expect("guide cleanup");
}
