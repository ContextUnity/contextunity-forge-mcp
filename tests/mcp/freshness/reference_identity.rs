use super::*;

#[test]
fn document_reference_identity_respects_workspace_and_match_tiers() {
    let mut regressions = Vec::new();
    let identity_ws = Workspace::with_files(&[]);
    let linked = Workspace::with_files(&[]);
    identity_ws.write("forge-mcp.yaml", &format!("roots: [.]\ndocs: [docs]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [src, frontend]\n    docs: [docs]\n", linked.root().display()));
    identity_ws.write(
        "primary.py",
        "def validate(): return 1\ndef primary_only(): return 2\n",
    );
    identity_ws.write("phantomcomponent/unit.py", "def marker(): pass\n");
    identity_ws.write(
        "docs/PhantomDoc.md",
        "# PhantomDoc\nA document is not a code symbol.\n",
    );
    identity_ws.write(
        "docs/context-primary.md",
        "# Primary\nUse `validate`, `linked_only`, `PhantomDoc`, and `phantomcomponent`.\n",
    );
    identity_ws.write(
        "docs/cross-workspace.md",
        "# Cross workspace\n[`validate`](../[library]/src/validate.py)\n",
    );
    linked.write("src/validate.py", "# validation module\n");
    linked.write("src/validate/__init__.py", "# validation package\n");
    linked.write("src/validate/validate.py", "# nested validation module\n");
    linked.write(
        "src/header.py",
        "class GridViewHeader:\n    content: str = ''\ndef linked_only(): return 3\n",
    );
    linked.write(
        "frontend/header.ts",
        "export interface GridViewHeader {\n  content: string;\n}\n",
    );
    linked.write(
        "docs/context-linked.md",
        "# Linked\nUse `validate` and `primary_only`.\n",
    );
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
    let mut identity_client = McpTestHarness::new(&identity_ws).expect("MCP initialize");
    for (path, expected_paths, candidate_paths) in [
        ("docs/context-primary.md", vec!["primary.py"], vec![]),
        (
            "[library]/docs/context-linked.md",
            vec![],
            vec![
                "[library]/src/validate.py",
                "[library]/src/validate/__init__.py",
                "[library]/src/validate/validate.py",
            ],
        ),
        (
            "docs/cross-workspace.md",
            vec!["[library]/src/validate.py"],
            vec![],
        ),
    ] {
        let result = identity_client
            .payload(
                "get_doc",
                json!({"path_or_id":path,"detail":"full","limit":100}),
            )
            .expect("MCP tool should succeed");
        let actual_paths = crate::json_array(&result["referenced_symbols"]["items"])
            .iter()
            .filter_map(|item| item["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if actual_paths != expected_paths.into_iter().collect() {
            regressions.push(format!(
                "document workspace/path eligibility changed verified targets for {path}: {result}"
            ));
        }
        let candidates = result["referenced_symbol_candidates"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let actual_candidates = candidates
            .iter()
            .filter_map(|item| item["target"]["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if actual_candidates != candidate_paths.into_iter().collect()
            || result["referenced_symbol_candidates"]["total"].as_u64()
                != Some(candidates.len() as u64)
        {
            regressions.push(format!("only eligible ambiguous selected-tier targets must remain discoverable for {path}: {result}"));
        }
    }
    let header = identity_client
        .document(
            "[library]/docs/header.md",
            json!({"section":"Header","detail":"full","limit":100}),
        )
        .expect("MCP tool should succeed");
    let header_candidates = header["referenced_symbol_candidates"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let header_paths = header_candidates
        .iter()
        .filter_map(|item| item["target"]["path"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        !(header["referenced_symbols"]["total"] != 0
            || header["referenced_symbol_candidates"]["total"] != 2
            || header_paths
                != ["[library]/frontend/header.ts", "[library]/src/header.py"]
                    .into_iter()
                    .collect()
            || header_candidates.iter().any(|item| {
                item["source"]["path"] != "[library]/docs/header.md"
                    || item["source"]["name"] != "GridViewHeader.content"
                    || item["source"]["line"] != 2
                    || !item["source"]["doc_id"].is_string()
                    || !item["target"]["id"].is_string()
                    || !item["target"]["qualname"].is_string()
                    || !item["target"]["kind"].is_string()
                    || item["target"]["name"] != "content"
                    || !item["target"]["line"].is_number()
            })),
        "ambiguous Python/TS fields require source-identified discovery, not verification or duplicate same-line claims: {header}"
    );
    let compact_header = identity_client
        .document(
            "[library]/docs/header.md",
            json!({"section":"Header","detail":"compact"}),
        )
        .expect("MCP tool should succeed");
    assert!(
        !(compact_header["referenced_symbol_candidates"]["omitted"] != true
            || !compact_header["referenced_symbol_candidates"]["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("detail=full"))),
        "compact get_doc must omit heavy claim discovery with a full-detail hint: {compact_header}"
    );
    let first_candidate = identity_client
        .document(
            "[library]/docs/header.md",
            json!({"section":"Header","detail":"full","limit":1}),
        )
        .expect("MCP tool should succeed");
    if let Some(generation) = first_candidate["referenced_symbol_candidates"]["generation"].as_str()
    {
        let next_candidate = identity_client.document("[library]/docs/header.md", json!({"section":"Header","detail":"full","limit":1,"offset":1,"generation":generation})).expect("MCP tool should succeed");
        let combined = [
            first_candidate["referenced_symbol_candidates"]["items"][0].clone(),
            next_candidate["referenced_symbol_candidates"]["items"][0].clone(),
        ];
        if first_candidate["referenced_symbol_candidates"]["total"] != 2
            || next_candidate["referenced_symbol_candidates"]["total"] != 2
            || first_candidate["referenced_symbol_candidates"]["next_offset"] != 1
            || combined.as_slice() != header_candidates.as_slice()
            || combined[0]["target"]["id"] == combined[1]["target"]["id"]
        {
            regressions.push(format!("candidate paging must preserve selected-section scope, complete total and deterministic distinct identities: first={first_candidate}, next={next_candidate}"));
        }
    } else {
        regressions.push(format!(
            "candidate continuation requires the normal snapshot generation: {first_candidate}"
        ));
    }
    let candidate_generation = first_candidate["referenced_symbol_candidates"]["generation"]
        .as_str()
        .unwrap();
    let candidate_markdown = response_text(
        &mut identity_client,
        "get_doc",
        json!({"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":1,"format":"markdown"}),
    );
    assert!(
        candidate_markdown.contains("Ambiguous symbol candidates (navigation only; unverified)")
    );
    for (field, heading) in [
        ("cites_doc", "Verified document citations"),
        ("cited_by_doc", "Verified incoming document citations"),
        ("referenced_symbols", "Verified referenced symbols"),
    ] {
        assert_eq!(
            candidate_markdown.contains(heading),
            first_candidate[field]["total"].as_u64().unwrap_or_default() > 0,
            "relation heading follows its page total ({field}): {candidate_markdown}"
        );
    }
    assert!(candidate_markdown.contains("Claim source"));
    assert!(candidate_markdown.contains("Possible target"));
    assert!(candidate_markdown.contains(candidate_generation));
    assert!(candidate_markdown.contains("**next_offset**: `1`"));
    assert!(candidate_markdown.contains("**has_more**: `true`"));
    assert!(
        candidate_markdown.contains("[\\[library\\]/docs/header.md](file://")
            && candidate_markdown.contains("[\\[library\\]/frontend/header.ts](file://"),
        "candidate source and target links resolve against their linked roots: {candidate_markdown}"
    );
    let stale_candidate_page = identity_client.request_value("tools/call", json!({"name":"get_doc","arguments":{"path_or_id":"[library]/docs/header.md","section":"Header","detail":"full","limit":1,"offset":1,"generation":"stale-candidate-generation"}})).expect("MCP transport");
    assert_eq!(
        stale_candidate_page["result"]["isError"], true,
        "{stale_candidate_page}"
    );
    let cli_document = identity_ws
        .query(&[
            "docs",
            "get",
            "[library]/docs/header.md",
            "--section",
            "Header",
        ])
        .expect("CLI should succeed");
    assert!(
        !(cli_document["referenced_symbol_candidates"]["total"] != 2
            || cli_document["referenced_symbol_candidates"]["items"]
                != header["referenced_symbol_candidates"]["items"]),
        "CLI docs get must expose full candidate discovery matching MCP default/full: CLI={cli_document}, MCP={header}"
    );
    let candidate_edges = identity_client.payload("code_map_analyze", json!({"target":"SELECT e.kind FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE p.path IN('[library]/docs/header.md','[library]/docs/context-linked.md') AND e.kind IN('documents','governs')","limit":100})).expect("MCP tool should succeed");
    assert!(
        !(candidate_edges["rows"]["total"] != 0),
        "candidate discovery must not establish documentary or governing graph edges: {candidate_edges}"
    );
    for (document, expected, present) in [
        (
            "exact-file",
            json!({"name":"target","path":"src/provider.rs"}),
            true,
        ),
        (
            "missing-file",
            json!({"name":"unique","kind":"function","path":"src/unique.rs"}),
            false,
        ),
        (
            "unique-file",
            json!({"name":"unique","kind":"function","path":"src/unique.rs"}),
            true,
        ),
        (
            "unlinked-unique",
            json!({"name":"unique","path":"src/unique.rs"}),
            true,
        ),
        ("ambiguous-unlinked", json!({"name":"shared_target"}), false),
        (
            "mixed-occurrences",
            json!({"name":"unique","path":"src/unique.rs"}),
            true,
        ),
        (
            "module-identity",
            json!({"name":"unique.rs","kind":"module","path":"src/unique.rs"}),
            true,
        ),
        (
            "path-tier",
            json!({"name":"target","path":"src/provider.rs"}),
            true,
        ),
        (
            "unsafe-file",
            json!({"name":"unique","path":"src/unique.rs"}),
            false,
        ),
        (
            "qualified",
            json!({"name":"live","path":"src/qualified.rs"}),
            true,
        ),
        (
            "qualified-priority",
            json!({"name":"live","path":"src/qualified.rs"}),
            true,
        ),
        (
            "suffix-only",
            json!({"name":"leaf","path":"src/qualified.rs"}),
            true,
        ),
    ] {
        let path = format!("docs/{document}.md");
        let result = identity_client
            .payload("get_doc", json!({"path_or_id":path,"limit":100}))
            .expect("MCP tool should succeed");
        let found = crate::json_array(&result["referenced_symbols"]["items"])
            .iter()
            .any(|symbol| {
                expected
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(field, value)| symbol[field] == *value)
            });
        assert_eq!(
            found, present,
            "{path}: expected claim {expected}, present={present}: {result}"
        );
    }
    let mixed_lines = identity_client.payload("code_map_analyze", json!({"target":"SELECT e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind='documents' AND p.path='docs/mixed-occurrences.md' ORDER BY e.line","limit":10})).expect("MCP tool should succeed");
    let mixed_lines = crate::json_array(&mixed_lines["rows"]["items"])
        .iter()
        .filter_map(|row| row["line"].as_u64())
        .collect::<Vec<_>>();
    assert_eq!(
        mixed_lines,
        [2],
        "a missing same-token link must neither erase nor authorize another path; line2 is the valid canonical source"
    );
    let global_exact = identity_client.payload("code_map_analyze", json!({"target":"SELECT n.kind,n.name,n.qualname,p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.qualname='target' ORDER BY p.path","limit":10})).expect("MCP tool should succeed");
    assert!(
        crate::json_array(&global_exact["rows"]["items"])
            .iter()
            .any(|row| row["path"] == "target.rs" && row["kind"] == "module"),
        "path-tier fixture needs the outside-file exact-qualname module: {global_exact}"
    );
    for (document, kind, count, target_line) in [
        ("qualified", "dead_doc_symbol", 0, None),
        ("multi-claim", "stale_doc_sections", 0, None),
        ("separate-claims", "stale_doc_sections", 2, None),
        ("qualified-priority", "stale_doc_sections", 2, Some(2)),
    ] {
        let result = identity_client
            .payload(
                "code_map_analyze",
                json!({"target":format!("docs/{document}.md"),"limit":100}),
            )
            .expect("MCP tool should succeed");
        let diagnostics = crate::json_array(&result["doc_diagnostics"]["items"])
            .iter()
            .filter(|item| item["kind"] == kind)
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), count, "{document} {kind}: {result}");
        if let Some(line) = target_line {
            assert!(
                diagnostics
                    .iter()
                    .all(|item| item["target"]["line"] == line),
                "exact qualified declaration wins over deeper suffix candidates for both spellings: {result}"
            );
        }
    }
    assert!(
        regressions.is_empty(),
        "Task5 documentary regressions: {regressions:#?}"
    );
    identity_client
        .finish()
        .expect("server must finish successfully");
}
