use super::*;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[test]
fn sighup_reloads_replaced_binary_in_same_pid_with_stdio_connected() {
    use std::os::unix::fs::MetadataExt;

    let ws = Workspace::new();
    let binary = ws.path(".forge/server");
    let replacement = ws.path(".forge/server-next");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_contextunity-forge-mcp"), &binary).unwrap();
    let mut client = StdioClient::with_binary(&ws, &binary);
    let pid = client.child().id();
    fs::copy(&binary, &replacement).unwrap();
    fs::rename(&replacement, &binary).unwrap();
    let new_inode = fs::metadata(&binary).unwrap().ino();

    let reload = Command::new(&binary).arg("reload").output().unwrap();
    assert!(
        reload.status.success(),
        "{}",
        String::from_utf8_lossy(&reload.stderr)
    );
    let response: Value = serde_json::from_slice(&reload.stdout).unwrap();
    assert_eq!(response["signaled_pids"], json!([pid]));

    let proc_exe = PathBuf::from(format!("/proc/{pid}/exe"));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while fs::metadata(&proc_exe).unwrap().ino() != new_inode {
        assert!(
            std::time::Instant::now() < deadline,
            "server did not exec the replacement"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let listed = client.request_value("tools/list", json!({}));
    assert!(listed.get("error").is_none(), "{listed}");
    let tools = listed["result"]["tools"].as_array().expect("{listed}");
    assert!(
        tools.iter().any(|tool| tool["name"] == "task_blackboard"),
        "{listed}"
    );
    assert_eq!(client.child().id(), pid);
}

#[test]
fn mcp_document_retrieval_and_explain_output_controls() {
    let ws = Workspace::new();
    ws.write(
        "main.rs",
        "pub fn target() {}\npub fn caller() { target(); }\n",
    );
    ws.write(
        "README.md",
        "# Contract\n\nThe `target` function preserves the documented contract.\n",
    );
    let mut client = StdioClient::new(&ws);
    let document = client.payload("get_doc", json!({"path_or_id":"README.md"}));
    assert!(document["sections"]["items"][0]["content"]
        .as_str()
        .unwrap()
        .contains("documented contract"));
    let outline = client.payload(
        "get_doc",
        json!({"path_or_id":"README.md","detail":"compact"}),
    );
    assert!(outline["sections"]["items"][0].get("content").is_none());
    assert_eq!(document["sections"]["total"], outline["sections"]["total"]);
    let mut explain = |arguments: Value| client.payload("code_map_explain", arguments);
    let with_docs = explain(json!({"selector":"target"}));
    assert!(with_docs["documents"]["total"].as_u64().unwrap() > 0);
    let without_docs = explain(json!({"selector":"target","show_doc":false}));
    assert!(without_docs.get("documents").is_none() || without_docs["documents"]["total"] == 0);
    assert_eq!(with_docs["coverage"], without_docs["coverage"]);
    assert_eq!(with_docs["incoming"], without_docs["incoming"]);
    for (alias, canonical, omitted) in [
        ("inbound", "incoming", "outgoing"),
        ("outbound", "outgoing", "incoming"),
    ] {
        let alias_result = explain(json!({"selector":"target","direction":alias}));
        let canonical_result = explain(json!({"selector":"target","direction":canonical}));
        assert_eq!(alias_result["direction"], canonical);
        assert_eq!(alias_result[canonical], canonical_result[canonical]);
        assert_eq!(alias_result[omitted]["omitted"], true);
    }

    drop(client);
    ws.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs/allowed]\n");
    ws.write(
        "src/main.rs",
        "pub fn target() {}\npub fn caller() { target(); }\npub fn grandcaller() { caller(); }\n",
    );
    ws.write("src/ambiguous.rs", "pub struct A;\nimpl A { pub fn shared() {} }\npub struct B;\nimpl B { pub fn shared() {} }\n");
    ws.write(
        "AGENTS.md",
        "# Agent rules\n- INV-AGENT: Preserve [`target`](src/main.rs#L1).\n",
    );
    ws.write("docs/guide.md", "---\ntitle: Navigation\ndoc_type: guide\n---\n# Navigation\n[Safety](rules.md#safety)\n[[extra#Other]]\n[Missing](missing.md)\n[Bad anchor](rules.md#absent)\n[Escape](../../outside.md)\n[[shared]]\n");
    let mut rules = String::from(
        "---\ntitle: Accepted rules\ndoc_type: adr\nstatus: accepted\n---\n# Safety\n",
    );
    for index in 0..6 {
        rules.push_str(&format!("- INV-D{index:02}: Preserve target contract {index} through [`target`](../src/main.rs#L1).\n"));
    }
    rules.push_str("[Navigation](guide.md#navigation)\n");
    ws.write("docs/rules.md", &rules);
    let mut indirect = String::from("# Other\n");
    for index in 0..6 {
        indirect.push_str(&format!("- INV-T{index:02}: Preserve caller contract {index} through [`caller`](../src/main.rs#L2).\n"));
    }
    indirect.push_str("- INV-DEEP: Preserve [`grandcaller`](../src/main.rs#L3).\n");
    ws.write("docs/extra.md", &indirect);
    ws.write("docs/left/shared.md", "# Shared\nLeft candidate.\n");
    ws.write("docs/right/shared.md", "# Shared\nRight candidate.\n");
    ws.write("docs/plans/proposed.md", "---\ntitle: Proposal\ndoc_type: plan\nstatus: proposed\n---\n# Proposal\n- INV-PLAN: Proposed [`target`](../../src/main.rs#L1) rule.\n");
    ws.write("docs/milestones/010-navigation.md", "---\nid: m-navigation\ntitle: Navigation contract\ndoc_type: contract\nstatus: active\n---\n# Contract\n- INV-MILESTONE: Preserve [`target`](../../src/main.rs#L1).\n");
    ws.write(
        "docs/unknown.md",
        "# Unspecified status\n- INV-UNKNOWN: Preserve [`target`](../src/main.rs#L1).\n",
    );
    ws.write("docs/range.md", "# Range\n[`target`](../src/main.rs#L1)\n[generic label](../src/main.rs#L2)\n[`target`](../src/main.rs#L0)\n[`target`](../src/main.rs#L999)\n[`shared`](../src/ambiguous.rs#L1)\n");
    ws.build();
    let mut client = StdioClient::new(&ws);

    let guide = client.payload("get_doc", json!({"path_or_id":"docs/guide.md","limit":100}));
    assert_eq!(guide["sections"]["items"][0]["line"], 5);
    let cited = guide["cites_doc"]["items"]
        .as_array()
        .expect("verified document citations");
    let paths = cited
        .iter()
        .map(|section| section["path"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        ["docs/rules.md", "docs/extra.md"].into_iter().collect()
    );
    let reciprocal = client.payload("get_doc", json!({"path_or_id":"docs/rules.md","limit":100}));
    assert!(reciprocal["cited_by_doc"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| section["path"] == "docs/guide.md"));
    let edges = client.payload("code_map_analyze", json!({"target":"SELECT e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind IN('cites_doc','cited_by_doc') AND p.path='docs/guide.md' ORDER BY e.kind,e.line","limit":100}));
    assert!(edges["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["kind"] == "cites_doc" && edge["line"] == 6));

    for (path, doc_type, status) in [
        ("AGENTS.md", "guide", Value::Null),
        ("docs/plans/proposed.md", "plan", json!("proposed")),
        (
            "docs/milestones/010-navigation.md",
            "contract",
            json!("active"),
        ),
        ("docs/unknown.md", "guide", Value::Null),
    ] {
        let document = client.payload("get_doc", json!({"path_or_id":path,"limit":100}));
        let section = &document["sections"]["items"][0];
        assert_eq!(section["doc_type"], doc_type);
        assert_eq!(
            section["status"], status,
            "unspecified is not accepted: {section}"
        );
        assert!(document["referenced_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/main.rs"));
    }
    let invariant_nodes = client.payload("code_map_analyze", json!({"target":"SELECT n.name,p.path,n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='invariant' ORDER BY n.name,p.path,n.line","limit":100}));
    assert!(invariant_nodes["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "INV-PLAN"
            && node["path"] == "docs/plans/proposed.md"
            && node["line"] == 7));
    let governance = client.payload("code_map_analyze", json!({"target":"SELECT e.kind,s.name source_name,t.name target_name,p.path,e.line FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes t ON t.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind IN('governs','governed_by') ORDER BY e.kind,s.name,t.name","limit":100}));
    let governance = governance["rows"]["items"].as_array().unwrap();
    for (kind, source, target) in [
        ("governs", "INV-D00", "target"),
        ("governed_by", "target", "INV-D00"),
    ] {
        assert!(governance.iter().any(|edge| edge["kind"] == kind
            && edge["source_name"] == source
            && edge["target_name"] == target
            && edge["path"] == "docs/rules.md"
            && edge["line"] == 7));
    }
    let range = client.payload("get_doc", json!({"path_or_id":"docs/range.md","limit":100}));
    assert!(
        range["referenced_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|symbol| symbol["name"] != "shared"),
        "ambiguous symbol claims are not verified references"
    );
    let governed = client.payload("code_map_analyze", json!({"target":"SELECT count(*) total FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE e.kind='governs' AND n.name='shared'"}));
    assert_eq!(governed["rows"]["items"][0]["total"], 0);

    for tool in ["code_map_inspect", "code_map_impact"] {
        let mut arguments = json!({"selector":"src/main.rs:target","limit":100});
        if tool == "code_map_inspect" {
            arguments["show_source"] = json!(false);
        }
        let result = client.payload(tool, arguments);
        let invariants = &result["governing_invariants"];
        let direct = invariants["direct"].as_array().unwrap();
        assert_eq!(direct.len(), 5);
        assert_eq!(
            direct
                .iter()
                .map(|rule| rule["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["INV-AGENT", "INV-D00", "INV-D01", "INV-D02", "INV-D03"]
        );
        assert_eq!(invariants["omitted_direct"], 5);
        for rule in direct {
            assert!(rule["rule"].as_str().unwrap().contains("Preserve"));
            assert!(!rule["rule"].as_str().unwrap().contains('\n'));
            assert!(rule["source"]["path"].is_string());
            assert!(rule["source"]["line"].as_u64().unwrap() > 0);
        }
        assert_eq!(
            invariants["transitive_ids"],
            json!(["INV-T00", "INV-T01", "INV-T02", "INV-T03", "INV-T04"])
        );
        assert_eq!(invariants["omitted_transitive"], 1);
        assert!(
            !invariants.to_string().contains("INV-DEEP"),
            "one hop is not arbitrary transitive closure"
        );
        let omitted = invariants["omitted_sources"].as_array().unwrap();
        assert!(!omitted.is_empty() && omitted.len() <= 5);
        for reference in omitted {
            assert!(reference["id"].is_string());
            let document =
                client.payload("get_doc", json!({"path_or_id":reference["source"]["path"]}));
            assert!(document["sections"]["total"].as_u64().unwrap() > 0);
        }
        let docs = &result[if tool == "code_map_inspect" {
            "documented_in"
        } else {
            "affected_docs"
        }];
        assert!(docs["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|doc| doc["path"] == "docs/rules.md"));
    }
    let expected = client.payload("get_doc", json!({"path_or_id":"docs/rules.md"}));
    let cli = crate::common::mcp_client::run_cli(&ws, &["docs", "get", "docs/rules.md"]);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli: Value = serde_json::from_slice(&cli.stdout).unwrap();
    for response in [&cli, &expected] {
        assert!(response["referenced_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/main.rs"));
    }
    assert_eq!(
        cli["referenced_symbols"]["total"],
        expected["referenced_symbols"]["total"]
    );

    let broken = client.payload(
        "code_map_analyze",
        json!({"target":"docs/guide.md","limit":100}),
    );
    let diagnostics = broken["doc_diagnostics"]["items"].as_array().unwrap();
    assert!(diagnostics
        .iter()
        .any(|item| item["kind"] == "broken_doc_link" && item["line"] == 8));
    assert!(diagnostics
        .iter()
        .any(|item| item["kind"] == "broken_doc_link" && item["line"] == 9));
    ws.write("src/main.rs", "// shifted\n// still a valid source range\npub fn target() {}\npub fn caller() { target(); }\npub fn grandcaller() { caller(); }\n");
    ws.delta(&["src/main.rs"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let stale = client.payload(
        "code_map_analyze",
        json!({"target":"docs/range.md","limit":100}),
    );
    let stale = stale["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .collect::<Vec<_>>();
    assert_eq!(
        stale.len(),
        1,
        "generic, ambiguous, and out-of-bounds claims cannot prove stale source: {stale:?}"
    );
    assert_eq!(stale[0]["reason"], "documented_symbol_range_mismatch");
    assert_eq!(stale[0]["claim"]["target_path"], "src/main.rs");
    assert_eq!(stale[0]["claim"]["start_line"], 1);
    assert_eq!(stale[0]["claim"]["end_line"], 1);
    assert_eq!(stale[0]["target"]["path"], "src/main.rs");
    assert_eq!(stale[0]["target"]["line"], 3);
    assert_eq!(stale[0]["target"]["end_line"], 3);
    assert_eq!(stale[0]["line"], 2);
    ws.write(
        "docs/range.md",
        "# Range\n[`target`](../src/main.rs#L3)\n[`target`](../src/main.rs#L2-L4)\n",
    );
    ws.delta(&["docs/range.md"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let clear = client.payload(
        "code_map_analyze",
        json!({"target":"docs/range.md","limit":100}),
    );
    assert!(!clear["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "stale_doc_sections"));
    ws.write("src/main.rs", "pub fn target_renamed() {}\npub fn caller() { target_renamed(); }\npub fn grandcaller() { caller(); }\n");
    ws.delta(&["src/main.rs"]);
    invalidate_mcp_connection_cache(&ws.db()).unwrap();
    let dead = client.payload(
        "code_map_analyze",
        json!({"target":"docs/range.md","limit":100}),
    );
    let dead = dead["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "dead_doc_symbol")
        .unwrap();
    assert_eq!(dead["suggested_symbol"]["name"], "target_renamed");
    let document = client.payload("get_doc", json!({"path_or_id":"docs/range.md","limit":100}));
    assert!(
        !document["referenced_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "target_renamed"),
        "a suggestion is not a verified documentary claim"
    );
    let affected = client.payload(
        "code_map_impact",
        json!({"selector":"src/main.rs","limit":100}),
    );
    assert!(affected["affected_docs"]["items"].as_array().unwrap().iter()
        .any(|section| section["path"] == "docs/range.md"),
        "deleting a documented symbol must retain affected documentation by its verified file claim: {affected}");

    let mut regressions = Vec::new();
    let relation_page0 = client.payload(
        "get_doc",
        json!({"path_or_id":"docs/guide.md","limit":1,"offset":0}),
    );
    let relation_page1 = client.payload(
        "get_doc",
        json!({"path_or_id":"docs/guide.md","limit":1,"offset":1,"generation":relation_page0["cites_doc"]["generation"]}),
    );
    if relation_page0["cites_doc"]["total"] != 2
        || relation_page1["cites_doc"]["total"] != 2
        || relation_page0["cites_doc"]["items"][0]["path"]
            == relation_page1["cites_doc"]["items"][0]["path"]
    {
        regressions.push(format!(
            "relation continuation must preserve total and return the second cited document: page0={relation_page0}, page1={relation_page1}"
        ));
    }

    let cli_first = crate::common::mcp_client::run_cli(&ws, &["docs", "get", "docs/guide.md"]);
    if cli_first.status.success() {
        let cli_first: Value = serde_json::from_slice(&cli_first.stdout).unwrap();
        let mcp_first = client.payload("get_doc", json!({"path_or_id":"docs/guide.md"}));
        if cli_first["cites_doc"]["total"] != mcp_first["cites_doc"]["total"]
            || cli_first["cites_doc"]["items"] != mcp_first["cites_doc"]["items"]
        {
            regressions.push(format!(
                "CLI and MCP first-page relation projections differ: cli={cli_first}, mcp={mcp_first}"
            ));
        }
    } else {
        regressions.push(format!(
            "CLI docs get failed: {}",
            String::from_utf8_lossy(&cli_first.stderr)
        ));
    }

    let identity_ws = Workspace::new();
    let linked = Workspace::new();
    identity_ws.write("forge-mcp.yaml", &format!("roots: [.]\ndocs: [docs]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [src, frontend]\n    docs: [docs]\n", linked.root().display()));
    identity_ws.write("primary.py", "def validate(): return 1\ndef primary_only(): return 2\n");
    identity_ws.write("phantomcomponent/unit.py", "def marker(): pass\n");
    identity_ws.write("docs/PhantomDoc.md", "# PhantomDoc\nA document is not a code symbol.\n");
    identity_ws.write("docs/context-primary.md", "# Primary\nUse `validate`, `linked_only`, `PhantomDoc`, and `phantomcomponent`.\n");
    identity_ws.write("docs/cross-workspace.md", "# Cross workspace\n[`validate`](../[library]/src/validate.py)\n");
    linked.write("src/validate.py", "# validation module\n");
    linked.write("src/validate/__init__.py", "# validation package\n");
    linked.write("src/validate/validate.py", "# nested validation module\n");
    linked.write("src/header.py", "class GridViewHeader:\n    content: str = ''\ndef linked_only(): return 3\n");
    linked.write("frontend/header.ts", "export interface GridViewHeader {\n  content: string;\n}\n");
    linked.write("docs/context-linked.md", "# Linked\nUse `validate` and `primary_only`.\n");
    linked.write("docs/header.md", "# Header\nUse `GridViewHeader.content` and `GridViewHeader.content`.\n## Other\nUse `GridViewHeader.content`.\n");
    identity_ws.write("src/main.rs", "pub fn target() {}\n");
    identity_ws.write("src/provider.rs", "pub fn target() {}\n");
    identity_ws.write("target.rs", "pub fn marker() {}\n");
    identity_ws.write("src/unique.rs", "pub fn unique() {}\n");
    identity_ws.write("provider.py", "def shared_target(): return 1\n");
    identity_ws.write("other.py", "def shared_target(): return 2\n");
    identity_ws.write(
        "src/qualified.rs",
        "pub mod inner {\n    pub fn live() {}\n}\npub mod outer {\n    pub mod src {\n        pub mod qualified {\n            pub mod inner {\n                pub fn live() {}\n            }\n        }\n    }\n}\npub mod suffix_only {\n    pub fn leaf() {}\n}\n",
    );
    identity_ws.write(
        "src/claims.rs",
        "// documented ranges\npub fn alpha() {}\npub fn beta() {}\n",
    );
    identity_ws.write(
        "docs/exact-file.md",
        "# Exact file\n[`target`](../src/provider.rs)\n",
    );
    identity_ws.write(
        "docs/missing-file.md",
        "# Missing file\n[`unique`](../src/absent.rs#L1)\n",
    );
    identity_ws.write(
        "docs/unique-file.md",
        "# Exact unique\n[`unique`](../src/unique.rs)\n",
    );
    identity_ws.write(
        "docs/unlinked-unique.md",
        "# Unlinked unique\nUse `unique` in this sentence.\n",
    );
    identity_ws.write(
        "docs/ambiguous-unlinked.md",
        "# Ambiguous unlinked\nUse `shared_target` without a file selector.\n",
    );
    identity_ws.write(
        "docs/mixed-occurrences.md",
        "# Mixed occurrences\n[`unique`](../src/unique.rs)\n[`unique`](../src/absent.rs#L1)\nUse `unique` here too.\n",
    );
    identity_ws.write(
        "docs/module-identity.md",
        "# Module identity\n[`src.unique`](../src/unique.rs)\n",
    );
    identity_ws.write(
        "docs/path-tier.md",
        "# Path tier\n[`target`](../src/provider.rs#L1)\n",
    );
    identity_ws.write(
        "docs/unsafe-file.md",
        "# Unsafe path\n[`unique`](../../outside.rs#L1)\n",
    );
    identity_ws.write(
        "docs/qualified.md",
        "# Qualified\n[`src.qualified.inner.live`](../src/qualified.rs#L2)\n",
    );
    identity_ws.write(
        "docs/qualified-priority.md",
        "# Qualified priority\n[`src.qualified.inner.live`](../src/qualified.rs#L1)\n[`src::qualified::inner::live`](../src/qualified.rs#L1)\n",
    );
    identity_ws.write(
        "docs/suffix-only.md",
        "# Suffix only\n[`suffix_only.leaf`](../src/qualified.rs)\n[`suffix_only::leaf`](../src/qualified.rs)\n",
    );
    identity_ws.write(
        "docs/multi-claim.md",
        "# Multi claim\n[`alpha` and `beta`](../src/claims.rs#L1)\n",
    );
    identity_ws.write(
        "docs/separate-claims.md",
        "# Separate claims\n[`alpha`](../src/claims.rs#L1) [`beta`](../src/claims.rs#L1)\n",
    );
    identity_ws.build();
    let mut identity_client = StdioClient::new(&identity_ws);
    for (path, expected_paths, candidate_paths) in [
        ("docs/context-primary.md", vec!["primary.py"], vec![]),
        ("[library]/docs/context-linked.md", vec![], vec!["[library]/src/validate.py", "[library]/src/validate/__init__.py", "[library]/src/validate/validate.py"]),
        ("docs/cross-workspace.md", vec!["[library]/src/validate.py"], vec![]),
    ] {
        let result = identity_client.payload("get_doc", json!({"path_or_id":path,"detail":"full","limit":100}));
        let actual_paths = result["referenced_symbols"]["items"].as_array().unwrap().iter()
            .filter_map(|item| item["path"].as_str()).collect::<std::collections::BTreeSet<_>>();
        if actual_paths != expected_paths.into_iter().collect() {
            regressions.push(format!("document workspace/path eligibility changed verified targets for {path}: {result}"));
        }
        let candidates = result["referenced_symbol_candidates"]["items"].as_array().cloned().unwrap_or_default();
        let actual_candidates = candidates.iter().filter_map(|item| item["target"]["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if actual_candidates != candidate_paths.into_iter().collect()
            || result["referenced_symbol_candidates"]["total"].as_u64() != Some(candidates.len() as u64)
        {
            regressions.push(format!("only eligible ambiguous selected-tier targets must remain discoverable for {path}: {result}"));
        }
    }
    let header = identity_client.payload("get_doc", json!({"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":100}));
    let header_candidates = header["referenced_symbol_candidates"]["items"].as_array().cloned().unwrap_or_default();
    let header_paths = header_candidates.iter().filter_map(|item| item["target"]["path"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if header["referenced_symbols"]["total"] != 0
        || header["referenced_symbol_candidates"]["total"] != 2
        || header_paths != ["[library]/frontend/header.ts", "[library]/src/header.py"].into_iter().collect()
        || header_candidates.iter().any(|item| item["source"]["path"] != "[library]/docs/header.md"
            || item["source"]["name"] != "GridViewHeader.content" || item["source"]["line"] != 2
            || !item["source"]["doc_id"].is_string() || !item["target"]["id"].is_string()
            || !item["target"]["qualname"].is_string() || !item["target"]["kind"].is_string()
            || item["target"]["name"] != "content" || !item["target"]["line"].is_number())
    {
        regressions.push(format!("ambiguous Python/TS fields require source-identified discovery, not verification or duplicate same-line claims: {header}"));
    }
    let compact_header = identity_client.payload("get_doc", json!({"path_or_id":"[library]/docs/header.md","section":"Header","detail":"compact"}));
    if compact_header["referenced_symbol_candidates"]["omitted"] != true
        || !compact_header["referenced_symbol_candidates"]["hint"].as_str().is_some_and(|hint| hint.contains("detail=full"))
    {
        regressions.push(format!("compact get_doc must omit heavy claim discovery with a full-detail hint: {compact_header}"));
    }
    let first_candidate = identity_client.payload("get_doc", json!({"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":1}));
    if let Some(generation) = first_candidate["referenced_symbol_candidates"]["generation"].as_str() {
        let next_candidate = identity_client.payload("get_doc", json!({"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":1,"offset":1,"generation":generation}));
        let combined = [first_candidate["referenced_symbol_candidates"]["items"][0].clone(), next_candidate["referenced_symbol_candidates"]["items"][0].clone()];
        if first_candidate["referenced_symbol_candidates"]["total"] != 2
            || next_candidate["referenced_symbol_candidates"]["total"] != 2
            || first_candidate["referenced_symbol_candidates"]["next_offset"] != 1
            || combined.as_slice() != header_candidates.as_slice()
            || combined[0]["target"]["id"] == combined[1]["target"]["id"]
        {
            regressions.push(format!("candidate paging must preserve selected-section scope, complete total and deterministic distinct identities: first={first_candidate}, next={next_candidate}"));
        }
    } else {
        regressions.push(format!("candidate continuation requires the normal snapshot generation: {first_candidate}"));
    }
    let stale_candidate_page = identity_client.request_value("tools/call", json!({"name":"get_doc","arguments":{"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":1,"offset":1,"generation":"stale-candidate-generation"}}));
    assert_eq!(stale_candidate_page["result"]["isError"], true, "{stale_candidate_page}");
    let cli = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .arg("--root").arg(identity_ws.root())
        .args(["docs", "get", "[library]/docs/header.md", "--section", "Header"])
        .output().unwrap();
    assert!(cli.status.success(), "CLI get_doc failed: {}", String::from_utf8_lossy(&cli.stderr));
    let cli_document: Value = serde_json::from_slice(&cli.stdout).unwrap();
    if cli_document["referenced_symbol_candidates"]["total"] != 2
        || cli_document["referenced_symbol_candidates"]["items"] != header["referenced_symbol_candidates"]["items"]
    {
        regressions.push(format!("CLI docs get must expose full candidate discovery matching MCP default/full: CLI={cli_document}, MCP={header}"));
    }
    let candidate_edges = identity_client.payload("code_map_analyze", json!({"target":"SELECT e.kind FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE p.path IN('[library]/docs/header.md','[library]/docs/context-linked.md') AND e.kind IN('documents','governs')","limit":100}));
    if candidate_edges["rows"]["total"] != 0 {
        regressions.push(format!("candidate discovery must not establish documentary or governing graph edges: {candidate_edges}"));
    }
    let exact_file = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/exact-file.md","limit":100}),
    );
    if !exact_file["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/provider.rs")
    {
        regressions.push(format!(
            "explicit source path failed to select its symbol despite a global name collision: {exact_file}"
        ));
    }
    let missing_file = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/missing-file.md","limit":100}),
    );
    if missing_file["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| {
            symbol["name"] == "unique"
                && symbol["kind"] == "function"
                && symbol["path"] == "src/unique.rs"
        })
    {
        regressions.push(format!(
            "explicit missing path fell back to a different file: {missing_file}"
        ));
    }
    let unique_file = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/unique-file.md","limit":100}),
    );
    if !unique_file["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| {
            symbol["name"] == "unique"
                && symbol["kind"] == "function"
                && symbol["path"] == "src/unique.rs"
        })
    {
        regressions.push(format!(
            "explicit same-file declaration lost to the containing module's suffix: {unique_file}"
        ));
    }
    let unlinked_unique = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/unlinked-unique.md","limit":100}),
    );
    if !unlinked_unique["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "unique" && symbol["path"] == "src/unique.rs")
    {
        regressions.push(format!(
            "unlinked exact declared-name occurrence lost to the same-file module suffix: {unlinked_unique}"
        ));
    }
    let ambiguous_unlinked = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/ambiguous-unlinked.md","limit":100}),
    );
    if ambiguous_unlinked["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "shared_target")
    {
        regressions.push(format!(
            "an unlinked exact-name collision must remain unresolved: {ambiguous_unlinked}"
        ));
    }
    let mixed_occurrences = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/mixed-occurrences.md","limit":100}),
    );
    let mixed_lines = identity_client.payload(
        "code_map_analyze",
        json!({"target":"SELECT e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind='documents' AND p.path='docs/mixed-occurrences.md' ORDER BY e.line","limit":10}),
    );
    let mixed_lines = mixed_lines["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["line"].as_u64())
        .collect::<Vec<_>>();
    if !mixed_occurrences["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "unique" && symbol["path"] == "src/unique.rs")
        || mixed_lines != [2]
    {
        regressions.push(format!(
            "a missing same-token link must not erase or authorize another path; edges store one row per document-section/symbol, so line 2 is the valid canonical source line, got {mixed_lines:?}: {mixed_occurrences}"
        ));
    }
    let module_identity = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/module-identity.md","limit":100}),
    );
    if !module_identity["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| {
            symbol["name"] == "unique.rs"
                && symbol["kind"] == "module"
                && symbol["path"] == "src/unique.rs"
        })
    {
        regressions.push(format!(
            "exact containing-module identity was replaced by the same-basename declaration: {module_identity}"
        ));
    }
    let path_tier = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/path-tier.md","limit":100}),
    );
    let global_exact = identity_client.payload(
        "code_map_analyze",
        json!({"target":"SELECT n.kind,n.name,n.qualname,p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.qualname='target' ORDER BY p.path","limit":10}),
    );
    if !global_exact["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["path"] == "target.rs" && row["kind"] == "module")
    {
        regressions.push(format!(
            "path-tier fixture did not contain the outside-file exact-qualname module: {global_exact}"
        ));
    }
    if !path_tier["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "target" && symbol["path"] == "src/provider.rs")
    {
        regressions.push(format!(
            "a stronger global qualified-name tier suppressed the eligible lower tier in the explicit file: {path_tier}"
        ));
    }
    let unsafe_file = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/unsafe-file.md","limit":100}),
    );
    if unsafe_file["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "unique" && symbol["path"] == "src/unique.rs")
    {
        regressions.push(format!(
            "an unsafe explicit path fell back to a workspace symbol: {unsafe_file}"
        ));
    }
    let qualified = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/qualified.md","limit":100}),
    );
    if !qualified["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/qualified.rs" && symbol["name"] == "live")
    {
        regressions.push(format!(
            "qualified live reference did not bind to its file: {qualified}"
        ));
    }
    let qualified_diagnostics = identity_client.payload(
        "code_map_analyze",
        json!({"target":"docs/qualified.md","limit":100}),
    );
    if qualified_diagnostics["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "dead_doc_symbol")
    {
        regressions.push(format!(
            "live qualified target was reported dead: {qualified_diagnostics}"
        ));
    }
    let qualified_priority_refs = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/qualified-priority.md","limit":100}),
    );
    if !qualified_priority_refs["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "live" && symbol["path"] == "src/qualified.rs")
    {
        regressions.push(format!(
            "the exact qualified declaration did not produce a verified reference: {qualified_priority_refs}"
        ));
    }
    let qualified_priority = identity_client.payload(
        "code_map_analyze",
        json!({"target":"docs/qualified-priority.md","limit":100}),
    );
    let qualified_priority_stale = qualified_priority["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .collect::<Vec<_>>();
    if qualified_priority_stale.len() != 2
        || qualified_priority_stale
            .iter()
            .any(|item| item["target"]["line"] != 2)
    {
        regressions.push(format!(
            "the exact qualified target must win over a deeper suffix candidate for both dot and :: spellings: refs={qualified_priority_refs}, diagnostics={qualified_priority}"
        ));
    }
    let suffix_only = identity_client.payload(
        "get_doc",
        json!({"path_or_id":"docs/suffix-only.md","limit":100}),
    );
    if !suffix_only["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "leaf" && symbol["path"] == "src/qualified.rs")
    {
        regressions.push(format!(
            "legacy dotted/:: suffix identities stopped resolving: {suffix_only}"
        ));
    }
    let multi_claim = identity_client.payload(
        "code_map_analyze",
        json!({"target":"docs/multi-claim.md","limit":100}),
    );
    if multi_claim["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "stale_doc_sections")
    {
        regressions.push(format!(
            "multiple code labels in one link proved a unique stale range: {multi_claim}"
        ));
    }
    let separate_claims = identity_client.payload(
        "code_map_analyze",
        json!({"target":"docs/separate-claims.md","limit":100}),
    );
    let separate_stale = separate_claims["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .count();
    if separate_stale != 2 {
        regressions.push(format!(
            "separate one-symbol links should remain independently stale, got {separate_stale}: {separate_claims}"
        ));
    }
    assert!(
        regressions.is_empty(),
        "Task5 documentary regressions: {regressions:#?}"
    );
}
#[test]
fn all_mcp_tools_admit_large_inventory_snapshot_without_raising_query_limit() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {}\n");
    ws.write("docs/intro.md", "# Intro\n\nCurrent documentation.\n");
    let database = ws.db();
    db::writer::build(ws.root(), &database, None).unwrap();

    let conn = rusqlite::Connection::open(&database).unwrap();
    let inventory: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='inventory_snapshot'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let padded = format!("{inventory}{}", " ".repeat(8 * 1024 * 1024));
    conn.execute(
        "UPDATE metadata SET value=?1 WHERE key='inventory_snapshot'",
        [&padded],
    )
    .unwrap();
    contextunity_forge_mcp::core::commitments::seal(&conn).unwrap();
    drop(conn);

    let mut client = StdioClient::new(&ws);
    let calls = [
        ("code_map_overview", json!({"detail":"compact","limit":1})),
        ("code_map_inspect", json!({"selector":"main.rs:current"})),
        ("get_code_snippet", json!({"selector":"main.rs:current"})),
        (
            "code_map_search",
            json!({"pattern":"current","detail":"compact","limit":10}),
        ),
        ("code_map_tests", json!({"selector":"main.rs:current"})),
        (
            "code_map_impact",
            json!({"selector":"main.rs:current","depth":1}),
        ),
        ("code_map_explain", json!({"selector":"main.rs:current"})),
        ("code_map_query", json!({"operation":"overview"})),
        ("code_map_analyze", json!({"target":""})),
        (
            "code_map_prove_removal",
            json!({"selector":"main.rs:current"}),
        ),
        (
            "ast_grep_search",
            json!({"pattern":"pub fn current() {}","language":"rust","path":"main.rs"}),
        ),
        ("search_docs", json!({"query":"documentation"})),
        ("get_doc", json!({"path_or_id":"docs/intro.md"})),
        ("forge_guide", json!({"topic":"query"})),
        ("session_checkpoint", json!({"action":"list"})),
    ];
    for (name, arguments) in calls {
        let response =
            client.request_value("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        if name == "get_code_snippet" {
            let snippet: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert!(snippet.get("source").is_some(), "{snippet}");
            assert!(snippet.get("coverage").is_none(), "{snippet}");
            assert!(snippet.get("documents").is_none(), "{snippet}");
        }
        if name == "code_map_overview" {
            let overview: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(overview["counts"]["files"], 2);
            assert!(
                overview["freshness"].get("files_checked").is_none()
                    || overview["freshness"]["files_checked"] == 2
            );
        }
    }

    ws.write("main.rs", "pub fn current() { let value = 1; }\n");
    invalidate_mcp_connection_cache(&database).unwrap();
    let response = client.request_value(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact","limit":1}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let refreshed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(refreshed["freshness"]["refresh"], "delta");

    let server = Server::new(ws.root().to_path_buf(), database);
    server
        .read(|conn| {
            assert_eq!(
                conn.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH),
                8 * 1024 * 1024
            );
            Ok(Value::Null)
        })
        .unwrap();
}

#[test]
fn stdio_response_only_adapter_transitions_preserve_facts_and_continuations() {
    let ws = Workspace::new();
    ws.write(
        "src/README.md",
        "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n",
    );
    ws.write("src/helper.md", "# Helper\ninside\n");
    ws.write("outside.md", "# Outside\noutside\n");
    let mut client = StdioClient::new(&ws);
    let before = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
    let database = ws.db();
    let identity = db::cache::identity(&database).unwrap();
    let database_hash = || scanner::digest(&fs::read(&database).unwrap());
    let before_hash = database_hash();
    let generation = before["sections"]["generation"].clone();
    assert!(generation.is_string(), "{before}");
    assert_eq!(before["freshness"]["files_checked"], 3);
    let default_limit = before["sections"]["limit"].as_u64().unwrap();
    for (configuration, limit) in [
        (Some("response: {page_size: 1}\n"), 1),
        (Some("response: {page_size: 2}\n"), 2),
        (None, default_limit),
        (Some("{}\n"), default_limit),
        (None, default_limit),
    ] {
        if let Some(configuration) = configuration {
            ws.write("forge-mcp.yaml", configuration);
        } else {
            fs::remove_file(ws.path("forge-mcp.yaml")).unwrap();
        }
        let current = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
        assert_eq!(current["sections"]["limit"], limit);
        assert_eq!(
            current["sections"]["items"].as_array().unwrap().len(),
            limit.min(4) as usize
        );
        assert!(
            current["freshness"].get("refresh").is_none()
                || current["freshness"]["refresh"] == "none",
            "{configuration:?}: {current}"
        );
        assert_eq!(current["sections"]["generation"], generation);
        for key in ["corpus_hash", "output_root"] {
            if let Some(expected) = before["freshness"].get(key) {
                if let Some(actual) = current["freshness"].get(key) {
                    assert_eq!(actual, expected);
                }
            }
        }
        let continued = client.payload(
            "get_doc",
            json!({"path_or_id":"src/README.md","offset":1,"limit":1,"generation":generation}),
        );
        assert_eq!(continued["sections"]["offset"], 1);
        assert_eq!(continued["sections"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(continued["sections"]["generation"], generation);
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    for invalid in [
        "response: {page_size: 0}\n",
        "response: [\n",
        "response: {page_size: 2}\nroots: ['..']\n",
    ] {
        ws.write("forge-mcp.yaml", invalid);
        let error = client.request_value(
            "tools/call",
            json!({"name":"get_doc","arguments":{"path_or_id":"src/README.md"}}),
        );
        assert_eq!(error["result"]["isError"], true, "{invalid}: {error}");
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    ws.write("forge-mcp.yaml", "response: {page_size: 1}\n");
    let recovered = client.payload(
        "get_doc",
        json!({"path_or_id":"src/README.md","offset":1,"generation":generation}),
    );
    assert!(
        recovered["freshness"].get("refresh").is_none()
            || recovered["freshness"]["refresh"] == "none"
    );
    assert_eq!(recovered["sections"]["generation"], generation);
    assert_eq!(database_hash(), before_hash);

    let mut previous_generation = generation;
    for (settings, files) in [
        ("roots: [src]\n", 2),
        ("roots: [src]\nignore: [helper.md]\n", 1),
        ("roots: [src]\nignore: [helper.md]\nadapter_version: 2\n", 1),
    ] {
        ws.write(
            "forge-mcp.yaml",
            &format!("{settings}response: {{page_size: 1}}\n"),
        );
        let changed = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
        assert_eq!(
            changed["freshness"]["refresh"], "rebuild",
            "{settings}: {changed}"
        );
        assert_eq!(changed["freshness"]["files_checked"], files);
        assert_eq!(changed["sections"]["limit"], 1);
        assert_ne!(changed["sections"]["generation"], previous_generation);
        previous_generation = changed["sections"]["generation"].clone();
    }
}

#[cfg(unix)]
#[test]
fn server_terminates_when_parent_process_exits() {
    struct ServerCleanup(Option<i32>);
    impl Drop for ServerCleanup {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                // SAFETY: this is the server launched by this fixture; disarmed after observed exit.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
    let cases = [(false, false), (true, false)]
        .into_iter()
        .chain(cfg!(target_os = "linux").then_some((true, true)));
    for (initialized, reload) in cases {
        let ws = Workspace::new();
        let binary = if reload {
            let binary = ws.path(".forge/server");
            fs::create_dir_all(binary.parent().unwrap()).unwrap();
            fs::copy(env!("CARGO_BIN_EXE_contextunity-forge-mcp"), &binary).unwrap();
            binary
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        };
        let pid_file = ws.path("server.pid");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "\"$1\" --root \"$2\" serve <&0 & echo $! >\"$3\"; wait",
                "parent-fixture",
            ])
            .arg(&binary)
            .arg(ws.root())
            .arg(&pid_file);
        let mut client = if initialized {
            StdioClient::with_command(&mut command)
        } else {
            StdioClient::with_pending_initialization(&mut command)
        };
        let server_pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut cleanup = ServerCleanup(Some(server_pid));
        if initialized {
            let listed = client.request_value("tools/list", json!({}));
            assert!(listed["result"]["tools"].is_array(), "{listed}");
        }
        if reload {
            let replacement = ws.path(".forge/server-next");
            fs::copy(&binary, &replacement).unwrap();
            fs::rename(&replacement, &binary).unwrap();
            // Freeze between monitor ticks so launcher loss and reload are pending together.
            assert_eq!(unsafe { libc::kill(server_pid, libc::SIGSTOP) }, 0);
            #[cfg(target_os = "linux")]
            {
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                loop {
                    let stat = fs::read_to_string(format!("/proc/{server_pid}/stat")).unwrap();
                    if stat
                        .rsplit_once(") ")
                        .is_some_and(|(_, tail)| tail.starts_with('T'))
                    {
                        break;
                    }
                    assert!(std::time::Instant::now() < deadline, "server did not stop");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        // The test owns the pipe writer and keeps it open after the launcher dies.
        client.terminate_launcher();
        if reload {
            assert_eq!(unsafe { libc::kill(server_pid, libc::SIGHUP) }, 0);
            assert_eq!(unsafe { libc::kill(server_pid, libc::SIGCONT) }, 0);
        }
        client.wait_for_transport_close(Duration::from_secs(10));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            // A zombie has exited; kill(pid, 0) alone would misclassify it as running.
            #[cfg(target_os = "linux")]
            let exited = fs::read_to_string(format!("/proc/{server_pid}/stat"))
                .map(|stat| {
                    stat.rsplit_once(") ")
                        .is_some_and(|(_, tail)| tail.starts_with('Z'))
                })
                .unwrap_or(true);
            #[cfg(not(target_os = "linux"))]
            let exited = unsafe { libc::kill(server_pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            if exited {
                cleanup.0 = None;
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "server closed stdout but kept running (initialized={initialized}, reload={reload})"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(unix)]
#[test]
fn connected_server_survives_inactivity_with_obsolete_timeout_setting() {
    let ws = Workspace::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    command
        .arg("--root")
        .arg(ws.root())
        .arg("serve")
        .env("CONTEXTUNITY_FORGE_MCP_IDLE_TIMEOUT", "1s");
    let mut client = StdioClient::with_command(&mut command);
    std::thread::sleep(Duration::from_secs(3));
    let listed = client.request_value("tools/list", json!({}));
    assert!(listed["result"]["tools"].is_array(), "{listed}");
}
