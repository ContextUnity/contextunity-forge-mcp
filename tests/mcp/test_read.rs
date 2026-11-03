//! Contract tests for the layered reader.

use crate::common::{mcp_client::StdioClient, Workspace};
use forge_core::model::{ReadBounds, ReadMode, ReadResult};
use forge_core::subsystems::{
    disabled_read_mode_error, disabled_tool_error, filter_subsystem_tools, SubsystemPreset,
    SubsystemRegistry,
};
use serde_json::json;

#[test]
fn read_models_and_core_tool_are_available_with_minimal_subsystems() {
    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    assert!(disabled_tool_error("forge_read", &registry).is_none());
    assert_eq!(
        filter_subsystem_tools(["forge_read", "get_doc", "code_map_explain"], &registry),
        ["forge_read"]
    );
    let result = ReadResult {
        path: "notes.txt".into(),
        mode: ReadMode::Lines,
        content: "hello".into(),
        truncated: false,
    };
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["mode"], "lines");
    assert_eq!(ReadBounds::default().start_line, None);
}

#[test]
fn structural_reader_modes_require_their_subsystems() {
    let mut registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    assert!(disabled_read_mode_error(ReadMode::Snippet, false, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Outline, true, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Lines, false, &registry).is_none());
    registry.enabled.insert("docs".into());
    assert!(disabled_read_mode_error(ReadMode::Outline, true, &registry).is_none());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_some());
    registry.enabled.insert("codemap".into());
    assert!(disabled_read_mode_error(ReadMode::Snippet, false, &registry).is_none());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_none());
}

#[test]
fn forge_read_serves_text_in_a_minimal_workspace() {
    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "profile: minimal\n");
    workspace.write("notes.txt", "first line\nsecond line\n");
    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    let enabled_tools = filter_subsystem_tools(["forge_read"], &registry);
    assert_eq!(enabled_tools, ["forge_read"]);
    let mut client = StdioClient::new(&workspace);
    let (_, response) = client.call("forge_read", json!({"path":"notes.txt", "mode":"lines"}));
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert!(response.to_string().contains("first line"), "{response}");
}

#[test]
fn forge_read_core_modes_and_cli_share_bounded_text_and_exports() {
    use crate::common::mcp_client::run_cli_json;
    let w = Workspace::new();
    w.write("forge-mcp.yaml", "profile: minimal\n");
    w.write("notes.txt", "first\r\nПривіт 42\r\nthird\r\nfourth\r\n");
    let mut client = StdioClient::new(&w);
    for (mode, extra, expected) in [
        (
            "raw",
            json!({"start_line":2,"line_count":1}),
            "Привіт 42\r\n",
        ),
        (
            "lines",
            json!({"start_line":2,"line_count":1}),
            "2: Привіт 42\r\n",
        ),
        (
            "search",
            json!({"pattern":"Привіт","context":0}),
            "2: Привіт 42\r\n",
        ),
        (
            "search",
            json!({"pattern":"[0-9]+$","regex":true,"context":0}),
            "",
        ),
        (
            "search",
            json!({"pattern":"[0-9]+","regex":true,"context":0}),
            "2: Привіт 42\r\n",
        ),
    ] {
        let mut args = extra;
        args["path"] = json!("notes.txt");
        args["mode"] = json!(mode);
        let result = client.payload("forge_read", args);
        assert_eq!(result["content"], expected, "{result}");
        assert!(result.get("Hints").is_none(), "{result}");
    }
    let args = json!({"path":"notes.txt","mode":"raw","start_line":2,"line_count":1,"output_file":"exports/read.txt"});
    let mcp = client.payload("forge_read", args);
    assert_eq!(
        std::fs::read_to_string(w.path("exports/read.txt")).unwrap(),
        "Привіт 42\r\n"
    );
    let output = run_cli_json(
        &w,
        &[
            "read",
            "notes.txt",
            "--mode",
            "raw",
            "--start-line",
            "2",
            "--line-count",
            "1",
            "--output-file",
            "exports/read.txt",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        mcp
    );
    let (_, old) = client.call(
        &["get_", "code_snippet"].concat(),
        json!({"selector":"notes.txt"}),
    );
    assert_eq!(old["result"]["isError"], true);
    assert!(!w.db().exists(), "core reads must not create the index");
}

#[test]
fn forge_read_rejects_sensitive_binary_and_escaping_paths_before_export() {
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "profile: minimal\nsensitive: ['secrets/**', '.env*']\n",
    );
    w.write("notes.txt", "safe\n");
    w.write("secrets/token.txt", "secret\n");
    w.write(".env.local", "secret\n");
    w.write_bytes("binary.dat", b"abc\0def");
    let mut client = StdioClient::new(&w);
    for (args, error) in [
        (json!({"path":"../outside"}), "relative"),
        (json!({"path":"/etc/passwd"}), "relative"),
        (
            json!({"path":"secrets/token.txt","output_file":"leaked.txt"}),
            "SENSITIVE_FILE",
        ),
        (json!({"path":".env.local"}), "SENSITIVE_FILE"),
        (
            json!({"path":"binary.dat","output_file":"leaked.txt"}),
            "BINARY_FILE",
        ),
        (
            json!({"path":"notes.txt","output_file":"../escaped.txt"}),
            "relative",
        ),
        (
            json!({"path":"notes.txt","output_file":".env.export"}),
            "SENSITIVE_FILE",
        ),
        (
            json!({"path":"notes.txt","mode":"snippet"}),
            "FEATURE_DISABLED",
        ),
        (
            json!({"path":"notes.txt","mode":"outline"}),
            "FEATURE_DISABLED",
        ),
        (
            json!({"path":"notes.txt","mode":"search","pattern":"[","regex":true}),
            "regex",
        ),
    ] {
        let (_, response) = client.call("forge_read", args);
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(response.to_string().contains(error), "{response}");
    }
    assert!(!w.path("leaked.txt").exists());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(w.path("notes.txt"), w.path("alias.txt")).unwrap();
        for args in [
            json!({"path":"alias.txt"}),
            json!({"path":"notes.txt","output_file":"alias.txt"}),
        ] {
            let (_, response) = client.call("forge_read", args);
            assert_eq!(response["result"]["isError"], true, "{response}");
        }
    }
}

#[test]
fn forge_read_delta_uses_real_git_and_rejects_option_baselines() {
    let w = Workspace::new();
    w.write("forge-mcp.yaml", "profile: minimal\n");
    w.write("notes.txt", "before\n");
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Reader",
            "-c",
            "user.email=reader@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ],
    ] {
        assert!(forge_core::git::git_command(w.root(), &args)
            .unwrap()
            .status
            .success());
    }
    w.write("notes.txt", "after\n");
    let mut client = StdioClient::new(&w);
    let result = client.payload("forge_read", json!({"path":"notes.txt","mode":"delta"}));
    let diff = result["content"].as_str().unwrap();
    assert!(
        diff.contains("-before") && diff.contains("+after"),
        "{result}"
    );
    let (_, response) = client.call(
        "forge_read",
        json!({"path":"notes.txt","mode":"delta","baseline":"--output=escaped"}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
}

#[test]
fn forge_read_delta_never_exposes_sensitive_sibling_through_git_pathspec() {
    use crate::common::mcp_client::run_cli_json;
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "profile: minimal\nsensitive: ['secrets/**']\n",
    );
    let paths = [
        ":(glob)**",
        "*",
        "?",
        "[ab]",
        ":(literal)secrets/token.txt",
        "--option.txt",
    ];
    for (index, path) in paths.iter().enumerate() {
        w.write(path, &format!("safe-before-{index}\n"));
    }
    w.write("secrets/token.txt", "old-secret\n");
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Reader",
            "-c",
            "user.email=reader@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ],
    ] {
        assert!(forge_core::git::git_command(w.root(), &args)
            .unwrap()
            .status
            .success());
    }
    for (index, path) in paths.iter().enumerate() {
        w.write(path, &format!("safe-after-{index}\n"));
    }
    w.write("secrets/token.txt", "new-secret\n");
    let mut client = StdioClient::new(&w);
    for (index, path) in paths.iter().enumerate() {
        let export = format!("exports/delta-{index}.txt");
        let result = client.payload(
            "forge_read",
            json!({"path":path,"mode":"delta","output_file":export}),
        );
        let content = result["content"].as_str().unwrap();
        assert!(
            !content.contains("old-secret") && !content.contains("new-secret"),
            "sensitive sibling leaked: {result}"
        );
        assert!(
            content.contains(&format!("-safe-before-{index}"))
                && content.contains(&format!("+safe-after-{index}")),
            "literal file omitted: {result}"
        );
        assert_eq!(
            content.matches("diff --git ").count(),
            1,
            "delta broadened beyond the admitted file: {content}"
        );
        assert_eq!(std::fs::read_to_string(w.path(&export)).unwrap(), content);
        let output = run_cli_json(
            &w,
            &["read", "--mode", "delta", "--baseline", "HEAD", "--", path],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            cli["content"], result["content"],
            "CLI/MCP delta diverged for {path}"
        );
    }
}

#[test]
fn forge_read_delta_uses_the_admitted_linked_git_context() {
    use crate::common::mcp_client::run_cli_json;
    let primary = Workspace::new();
    let linked = Workspace::new();
    primary.write(
        "forge-mcp.yaml",
        &format!("profile: minimal\nsensitive: ['**/secrets/**']\nlinked_workspaces:\n  - name: linked\n    path: {}\n", linked.root().display()),
    );
    let paths = ["nested/notes.txt", "nested/:(glob)**"];
    for path in paths {
        linked.write(path, "linked-before\n");
        primary.write(format!("[linked]/{path}"), "decoy-before\n");
    }
    linked.write("nested/secrets/token.txt", "secret-before\n");
    for workspace in [&primary, &linked] {
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Reader",
                "-c",
                "user.email=reader@example.invalid",
                "commit",
                "-qm",
                "baseline",
            ],
        ] {
            assert!(forge_core::git::git_command(workspace.root(), &args)
                .unwrap()
                .status
                .success());
        }
    }
    for (workspace, tag) in [(&primary, "primary-only"), (&linked, "linked-only")] {
        assert!(
            forge_core::git::git_command(workspace.root(), &["tag", tag])
                .unwrap()
                .status
                .success()
        );
    }
    for path in paths {
        linked.write(path, "linked-after\n");
        primary.write(format!("[linked]/{path}"), "decoy-after\n");
    }
    linked.write("nested/secrets/token.txt", "secret-after\n");
    let mut client = StdioClient::new(&primary);
    let mut failures = Vec::new();
    for path in paths {
        let alias = format!("[linked]/{path}");
        let raw = client.payload("forge_read", json!({"path":alias,"mode":"raw"}));
        assert_eq!(raw["content"], "linked-after\n");
        for baseline in [None, Some("linked-only")] {
            let mut request = json!({"path":alias,"mode":"delta"});
            let mut args = vec!["read", "--mode", "delta"];
            if let Some(baseline) = baseline {
                request["baseline"] = json!(baseline);
                args.extend(["--baseline", baseline]);
            }
            args.extend(["--", &alias]);
            let (_, response) = client.call("forge_read", request);
            let output = run_cli_json(&primary, &args);
            let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
            // Exercise both real transports before reporting a runtime-red failure.
            for (transport, value) in [("MCP", response.to_string()), ("CLI", cli.to_string())] {
                if !value.contains("-linked-before")
                    || !value.contains("+linked-after")
                    || value.contains("decoy-")
                    || value.contains("secret-")
                    || value.matches("diff --git ").count() != 1
                {
                    failures.push(format!("{transport} {alias} {baseline:?}: {value}"));
                }
            }
            if !output.status.success() {
                failures.push(format!(
                    "CLI failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    for baseline in ["primary-only", "--output=escaped", "missing-revision"] {
        let (_, response) = client.call(
            "forge_read",
            json!({"path":"[linked]/nested/notes.txt","mode":"delta","baseline":baseline}),
        );
        let output = run_cli_json(
            &primary,
            &[
                "read",
                "[linked]/nested/notes.txt",
                "--mode",
                "delta",
                &format!("--baseline={baseline}"),
            ],
        );
        if response["result"]["isError"] != true || output.status.success() {
            failures.push(format!(
                "unavailable/unsafe baseline accepted: {baseline}: {response}"
            ));
        }
    }
    let (_, response) = client.call(
        "forge_read",
        json!({"path":"[linked]/nested/secrets/token.txt","mode":"delta"}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(response.to_string().contains("SENSITIVE_FILE"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn forge_read_implementation_hints_select_an_executable_type() {
    use crate::common::mcp_client::run_cli_json;
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules: {codemap: true}\nroots: [.]\n",
    );
    w.write("model.py", "class Base:\n    pass\n\nclass Child(Base):\n    pass\n\ndef standalone():\n    pass\n\nclass InvalidBase(standalone):\n    pass\n");
    let mut client = StdioClient::new(&w);
    for (arguments, expected) in [
        (json!({"path":"model.py","mode":"outline"}), false),
        (json!({"path":"model.py","mode":"lines"}), false),
        (json!({"path":"model.py","mode":"raw"}), false),
        (
            json!({"path":"model.py","mode":"search","pattern":"class"}),
            false,
        ),
        (json!({"selector":"model.py"}), false),
        (json!({"selector":"Base"}), true),
        (json!({"selector":"Child"}), false),
        (json!({"selector":"standalone"}), false),
    ] {
        let read = client.payload("forge_read", arguments);
        let hints = read["Hints"].as_str().unwrap_or("");
        let implementation = hints
            .split(';')
            .find(|call| call.contains("operation: \"implementations\""));
        assert_eq!(implementation.is_some(), expected, "{read}");
        if let Some(call) = implementation {
            let selector = call
                .split("selector: ")
                .nth(1)
                .unwrap()
                .split(", worktree:")
                .next()
                .unwrap();
            let selector: String = serde_json::from_str(selector).unwrap();
            let followed = client.payload(
                "code_map_query",
                json!({"operation":"implementations","selector":selector,"worktree":w.root()}),
            );
            assert_eq!(followed["nodes"]["total"], 1, "{followed}");
        }
    }
    for args in [
        vec!["read", "model.py", "--mode", "outline"],
        vec!["read", "model.py", "--selector", "Base"],
    ] {
        let output = run_cli_json(&w, &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            read["Hints"]
                .as_str()
                .unwrap_or("")
                .contains("operation: \"implementations\""),
            args.contains(&"Base"),
            "{read}"
        );
    }
}

#[test]
fn forge_read_docs_outline_keeps_hierarchy_and_filters_disabled_navigation() {
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules: {docs: true}\nroots: [.]\ndocs: [docs]\n",
    );
    w.write(
        "docs/guide.md",
        "# Guide\n\nText\n\n## Setup\n\n### Install\n\n```md\n## Code example\n```\n",
    );
    let mut client = StdioClient::new(&w);
    let result = client.payload(
        "forge_read",
        json!({"path":"docs/guide.md","mode":"outline"}),
    );
    let outline = result["content"].as_str().unwrap();
    assert!(
        outline.contains("# Guide")
            && outline.contains("## Setup")
            && outline.contains("### Install"),
        "{result}"
    );
    assert!(!outline.contains("Code example"), "{result}");
    let hints = result["Hints"].as_str().unwrap();
    assert!(
        hints.contains("get_doc(") && hints.contains("search_docs("),
        "{result}"
    );
    assert!(
        !hints.contains("code_map_") && !hints.contains("task_"),
        "{result}"
    );
}

#[test]
fn forge_read_codemap_outline_and_snippet_use_verified_graph_signals() {
    let w = Workspace::new();
    w.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules: {codemap: true}\nroots: [.]\n",
    );
    w.write(
        "service.py",
        "def compute():\n    return 42\n\ndef invoke():\n    return compute()\n",
    );
    w.write(
        "tests/test_service.py",
        "from service import compute\ndef test_compute():\n    assert compute() == 42\n",
    );
    let mut client = StdioClient::new(&w);
    let outline = client.payload("forge_read", json!({"path":"service.py","mode":"outline"}));
    assert!(
        outline["content"].as_str().unwrap().contains("compute"),
        "{outline}"
    );
    assert!(
        !outline["content"].as_str().unwrap().contains("return 42"),
        "{outline}"
    );
    let snippet = client.payload("forge_read", json!({"selector":"compute"}));
    assert_eq!(snippet["source"], "def compute():\n    return 42\n");
    let hints = snippet["Hints"].as_str().unwrap();
    assert!(
        hints.contains("code_map_explain(") && hints.contains("code_map_tests("),
        "{snippet}"
    );
    assert!(
        !hints.contains("get_doc") && !hints.contains("task_"),
        "{snippet}"
    );
    assert!(hints.lines().count() <= 3);
    assert!(snippet["graph_pulse"]["callers"].as_u64().unwrap() >= 2);
    assert_eq!(snippet["graph_pulse"]["covering_tests"], 1);
}

#[test]
fn forge_read_acdd_badges_and_hints_follow_the_unique_claimed_worktree() {
    use contextunity_forge_mcp::engine::tasks;
    for with_command in [false, true] {
        let w = Workspace::new();
        w.write("forge-mcp.yaml", "profile: minimal\nmodules: {acdd: true, blackboard: true}\nroots: [.]\ndocs: [docs]\nmilestones: [docs/milestones]\n");
        if with_command {
            let profile = forge_tasks::tasks::profile::load_for_workspace(w.root()).unwrap();
            let mut profile = serde_json::to_value(profile.as_ref()).unwrap();
            profile["gates"][0]["command"] = json!("reader-check");
            profile["commands"]["reader-check"] =
                json!({"command":"true","output":{"require_counts":false}});
            w.write(
                "reader-profile.yaml",
                &serde_yaml::to_string(&profile).unwrap(),
            );
            let mut config = std::fs::read_to_string(w.path("forge-mcp.yaml")).unwrap();
            config.push_str("acdd_profile: reader-profile.yaml\n");
            w.write("forge-mcp.yaml", &config);
        }
        w.write("allowed.txt", "allowed\n");
        w.write("other.txt", "other\n");
        w.write("docs/milestones/010-reader.md", "---\nid: m-reader\ntitle: Reader\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: read\ntarget: Read scoped text\nproof_policy: seam-test-first\nscope: [allowed.txt]\n```\n");
        let sync = crate::common::mcp_client::run_cli_json(
            &w,
            &["task", "sync", "--file", "docs/milestones/010-reader.md"],
        );
        assert!(
            sync.status.success(),
            "{}",
            String::from_utf8_lossy(&sync.stderr)
        );
        let task_id = "forge-mcp/m-reader:read";
        tasks::claim(
            w.root(),
            tasks::Claim {
                task_id: task_id.into(),
                stage: "contract".into(),
                worker_id: "reader-contract".into(),
                worktree: w.root().to_string_lossy().into_owned(),
                bundle: Some(false),
            },
        )
        .unwrap();
        let mut client = StdioClient::new(&w);
        let allowed = client.payload("forge_read", json!({"path":"allowed.txt"}));
        assert_eq!(allowed["write_scope"], "ALLOWED", "{allowed}");
        let outside = client.payload("forge_read", json!({"path":"other.txt"}));
        assert_eq!(outside["write_scope"], "OUT_OF_SCOPE", "{outside}");
        let hints = outside["Hints"].as_str().unwrap();
        assert!(
            hints.contains("extend_scope") && hints.contains("task_blackboard"),
            "{outside}"
        );
        assert!(
            !hints.contains("code_map_") && !hints.contains("get_doc"),
            "{outside}"
        );
        assert_eq!(hints.contains("verify"), with_command, "{outside}");
    }
}
