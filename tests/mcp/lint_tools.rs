use crate::common::{
    Workspace,
    mcp_client::{StdioClient, run_cli, run_cli_json},
};
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::lint,
    engine::languages,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs};

fn populate(workspace: &Workspace) -> BTreeSet<String> {
    let mut profiles = BTreeSet::new();
    for profile in languages::profiles() {
        let source = match profile.id() {
            "python" => "def\n",
            "rust" => "fn broken( {\n",
            "javascript" | "typescript" => "function broken( {\n",
            "vue" => "<script>function broken( {</script>\n",
            "proto" => "message Broken { invalid = ;\n",
            "html" => "<div attr=\"",
            "yaml" => "key: [one, two\n",
            "toml" => "key = [\n",
            "go" => "package main\nfunc broken( {\n",
            "java" => "class Broken { void broken( }\n",
            "csharp" => "class Broken { void Broken( }\n",
            "kotlin" => "fun broken( {\n",
            "php" => "<?php function broken( {\n",
            "ruby" => "def broken(\n",
            "c" | "cpp" => "void broken( {\n",
            language => panic!("add a syntax fixture for compiled language {language}"),
        };
        workspace.write(format!("src/broken.{}", profile.extensions()[0]), source);
        profiles.insert(profile.id().to_owned());
    }
    workspace.write(
        "README.md",
        "# Guide\nThis document is not syntax-checked.\n",
    );
    workspace.write("ignored.unknown", "This file has no compiled grammar.\n");
    profiles
}

#[test]
fn stored_syntax_lint_covers_compiled_profiles_without_mutating_index() {
    let workspace = Workspace::new();
    let expected = populate(&workspace);
    workspace.build();
    let conn = workspace.open();
    let before = fs::read(workspace.db()).unwrap();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(100), 0, None, None).unwrap();
    if expected.is_empty() {
        assert!(lint::syntax_paged(&conn, "", &options).is_err());
        return;
    }
    let result = lint::syntax_paged(&conn, "", &options).unwrap();
    let coverage = &result["coverage"];
    let actual: BTreeSet<_> = coverage["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["language"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(coverage["indexed_source_files"], expected.len());
    assert_eq!(coverage["other_indexed_files"], 1);
    assert_eq!(coverage["unindexed_files"], "not_enumerated");
    let diagnostics = result["diagnostics"]["items"].as_array().unwrap();
    let reported: BTreeSet<_> = diagnostics
        .iter()
        .map(|row| row["language"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(reported, expected, "{result}");
    for item in diagnostics {
        assert_eq!(item["rule_id"], "syntax.parse");
        assert_eq!(item["severity"], "error");
        assert!(item["line"].as_u64().unwrap() >= 1);
        assert!(!item["message"].as_str().unwrap().is_empty());
    }
    for target in [
        "README.md",
        "ignored.unknown",
        "not-indexed",
        "../outside",
        " / ",
        " src/../outside ",
        "SELECT * FROM nodes",
    ] {
        assert!(
            lint::syntax_paged(&conn, target, &options).is_err(),
            "{target}"
        );
    }
    let missing = lint::syntax_paged(&conn, "nonexistent.rs", &options)
        .unwrap_err()
        .to_string();
    assert_eq!(
        missing,
        "PATH_NOT_INDEXED: 'nonexistent.rs' was not found in the indexed inventory"
    );
    let error = lint::syntax_paged(&conn, "README.md", &options)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no indexed source files with a compiled language profile"),
        "{error}"
    );
    assert!(!error.contains("PATH_NOT_INDEXED"), "{error}");
    let unindexed = lint::syntax_paged(&conn, "ignored.unknown", &options)
        .unwrap_err()
        .to_string();
    assert_eq!(
        unindexed,
        "PATH_NOT_INDEXED: 'ignored.unknown' was not found in the indexed inventory"
    );
    assert_eq!(fs::read(workspace.db()).unwrap(), before);
}

#[test]
fn absence_of_stored_diagnostics_is_not_a_full_lint_success_claim() {
    let Some(profile) = languages::profiles().next() else {
        return;
    };
    let workspace = Workspace::new();
    let path = format!("source.{}", profile.extensions()[0]);
    workspace.write(&path, "\n");
    workspace.build();
    let conn = workspace.open();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    let result = lint::syntax_paged(&conn, &path, &options).unwrap();
    assert_eq!(result["status"], "no_stored_syntax_diagnostics");
    assert_eq!(result["diagnostics"]["total"], 0);
    assert_eq!(
        result["coverage"]["checks"],
        json!(["stored_parser_diagnostics"])
    );
    assert!(result["coverage"]["limitations"]
        .as_str()
        .unwrap()
        .contains("does not prove syntactic validity"));
}

fn analyze_call(client: &mut StdioClient, arguments: Value) -> Value {
    client.call("code_map_analyze", arguments).1
}

fn analyze_payload(client: &mut StdioClient, arguments: Value) -> Value {
    client.payload("code_map_analyze", arguments)
}

#[test]
fn mcp_lint_is_opt_in_bounded_and_rejects_incompatible_requests() {
    let workspace = Workspace::new();
    if populate(&workspace).is_empty() {
        return;
    }
    workspace.write("clean.py", "def clean(): return 1\n");
    let mut client = StdioClient::new(&workspace);
    let default_overview = client.payload("code_map_overview", json!({}));
    let empty_aspect_overview = client.payload("code_map_overview", json!({"aspects":[]}));
    for aspect in [
        "components",
        "counts",
        "languages",
        "coverage_by_workspace",
        "compiled_profiles",
        "metadata",
        "discovery",
    ] {
        assert_eq!(
            empty_aspect_overview[aspect], default_overview[aspect],
            "aspect {aspect}"
        );
    }
    assert!(default_overview.get("cycles").is_none());
    assert!(empty_aspect_overview.get("cycles").is_none());
    for (aspect, expected_facet) in [
        ("discovery", "workspace_root"),
        ("manifest_components", "manifest_components"),
        ("documentation_hubs", "documentation_hubs"),
        ("milestones", "active_milestones"),
    ] {
        let overview = client.payload("code_map_overview", json!({"aspects":[aspect]}));
        assert!(
            overview["discovery"].get(expected_facet).is_some(),
            "supported discovery aspect {aspect} omitted {expected_facet}: {overview}"
        );
    }

    let normal = analyze_payload(&mut client, json!({"target":""}));
    assert!(normal.get("mode").is_none());
    let first = analyze_payload(&mut client, json!({"target":"","lint":true,"limit":1}));
    assert_eq!(first["mode"], "lint");
    assert_eq!(first["diagnostics"]["items"].as_array().unwrap().len(), 1);
    let mut offset = first["diagnostics"]["next_offset"].clone();
    let generation = first["diagnostics"]["generation"].clone();
    let mut seen = first["diagnostics"]["items"].as_array().unwrap().clone();
    while !offset.is_null() {
        let next = analyze_payload(
            &mut client,
            json!({"target":"","lint":true,"limit":1,"offset":offset,"generation":generation}),
        );
        seen.extend(
            next["diagnostics"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        offset = next["diagnostics"]["next_offset"].clone();
    }
    assert_eq!(
        seen.len(),
        first["diagnostics"]["total"].as_u64().unwrap() as usize
    );
    let all = analyze_payload(&mut client, json!({"target":"","lint":true,"limit":100}));
    assert_eq!(json!(seen), all["diagnostics"]["items"]);
    for arguments in [
        json!({"target":"","lint":true,"include_cycles":true}),
        json!({"target":"","lint":true,"offset":1}),
        json!({"target":"","lint":true,"generation":"stale"}),
        json!({"target":"README.md","lint":true}),
        json!({"target":"ignored.unknown","lint":true}),
        json!({"target":"SELECT count(*) FROM nodes","lint":true}),
    ] {
        let result = analyze_call(&mut client, arguments);
        assert_eq!(result["result"]["isError"], true, "{result}");
    }
    let sql = analyze_payload(
        &mut client,
        json!({"target":"SELECT count(*) AS total FROM nodes"}),
    );
    assert!(sql["rows"]["items"][0]["total"].as_u64().unwrap() > 0);

    let mcp_clean = analyze_payload(&mut client, json!({"target":"clean.py"}));
    let cli_clean = run_cli_json(&workspace, &["query", "analyze", "clean.py"]);
    assert!(
        cli_clean.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_clean.stderr)
    );
    let cli_clean: Value = serde_json::from_slice(&cli_clean.stdout).unwrap();
    for field in [
        "scope",
        "total_errors",
        "total_unresolved",
        "total_external_imports",
    ] {
        assert_eq!(mcp_clean[field], cli_clean[field], "field {field}");
    }
    for page in ["errors", "resolution", "external_imports"] {
        assert_eq!(mcp_clean[page]["total"], cli_clean[page]["total"]);
        assert_eq!(mcp_clean[page]["items"], cli_clean[page]["items"]);
    }
    assert_eq!(mcp_clean["scope"], "file");
    assert_eq!(mcp_clean["total_errors"], 0);
    assert_eq!(mcp_clean["total_unresolved"], 0);
    assert_eq!(mcp_clean["total_external_imports"], 0);
    assert!(mcp_clean.get("top_files").is_none());
    assert!(cli_clean.get("top_files").is_none());

    for mode in ["diagnostics", "cycles"] {
        let expected_mode_error =
            format!("analysis target '{mode}' is interpreted as a path, not an analysis mode");
        let (_, mcp_mode_error) = client.call("code_map_analyze", json!({"target":mode}));
        assert_eq!(
            mcp_mode_error["result"]["isError"], true,
            "{mcp_mode_error}"
        );
        assert!(
            mcp_mode_error.to_string().contains(&expected_mode_error),
            "{mcp_mode_error}"
        );

        let cli_mode_error = run_cli(&workspace, &["query", "analyze", mode]);
        let cli_mode_stderr = String::from_utf8_lossy(&cli_mode_error.stderr);
        assert!(!cli_mode_error.status.success(), "{cli_mode_stderr}");
        assert!(
            cli_mode_stderr.contains(&expected_mode_error),
            "{cli_mode_stderr}"
        );
    }

    let expected_missing =
        "PATH_NOT_INDEXED: 'nonexistent.rs' was not found in the indexed inventory";
    let (_, missing_analyze) = client.call("code_map_analyze", json!({"target":"nonexistent.rs"}));
    let cli_missing = run_cli(&workspace, &["query", "analyze", "nonexistent.rs"]);
    let cli_missing_stderr = String::from_utf8_lossy(&cli_missing.stderr);
    assert!(
        missing_analyze["result"]["isError"] == true && !cli_missing.status.success(),
        "MCP: {missing_analyze}; CLI: {cli_missing_stderr}"
    );
    assert!(
        missing_analyze.to_string().contains(expected_missing)
            && cli_missing_stderr.contains(expected_missing),
        "MCP: {missing_analyze}; CLI: {cli_missing_stderr}"
    );

    let missing_lint = analyze_call(&mut client, json!({"target":"nonexistent.rs","lint":true}));
    assert_eq!(missing_lint["result"]["isError"], true, "{missing_lint}");
    assert!(
        missing_lint.to_string().contains(expected_missing),
        "{missing_lint}"
    );

    let clean_lint = analyze_payload(&mut client, json!({"target":"clean.py","lint":true}));
    assert_eq!(clean_lint["mode"], "lint");
    assert_eq!(clean_lint["status"], "no_stored_syntax_diagnostics");
    assert_eq!(clean_lint["diagnostics"]["total"], 0);

    let unsupported_aspect = client
        .call("code_map_overview", json!({"aspects":["unsupported"]}))
        .1;
    assert_eq!(
        unsupported_aspect["result"]["isError"], true,
        "{unsupported_aspect}"
    );
    for aspect in [
        "counts",
        "components",
        "languages",
        "coverage_by_workspace",
        "cycles",
        "compiled_profiles",
        "metadata",
        "discovery",
        "manifest_components",
        "documentation_hubs",
        "milestones",
    ] {
        assert!(
            unsupported_aspect.to_string().contains(aspect),
            "missing allowed overview aspect {aspect}: {unsupported_aspect}"
        );
    }
}
