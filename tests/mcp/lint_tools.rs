use crate::common::{mcp_client::StdioClient, Workspace};
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::lint,
    engine::languages,
};
use serde_json::{json, Value};
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
    let mut client = StdioClient::new(&workspace);
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
}
