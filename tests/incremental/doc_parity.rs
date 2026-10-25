#![cfg(feature = "lang-toml")]

use rusqlite::Connection;
use std::path::Path;

use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::reader,
};

fn doc_edges(db: &Path) -> Vec<(String, String, String)> {
    let conn = Connection::open(db).unwrap();
    let mut statement = conn
        .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind FROM edges WHERE kind IN ('documents','references_doc','cites_doc','cited_by_doc','governs','governed_by') ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn delta_document_suffix_links_match_cold_after_edit_and_restore() {
    let workspace = Workspace::new();
    workspace.write("settings.toml", "[server]\nlimits.cpu = 2\n");
    let original = "# Configuration\nUse `server.limits.cpu` to set the limit.\n";
    workspace.write("docs/guide.md", original);
    workspace.build();
    let baseline = doc_edges(&workspace.db());
    assert_eq!(
        baseline.len(),
        2,
        "expected reciprocal links to the TOML value"
    );

    for source in [
        "# Configuration\nUse `server.limits.cpu` to set the limit.\n\nDetails.\n",
        original,
    ] {
        workspace.write("docs/guide.md", source);
        workspace.delta(&["docs/guide.md"]);
        let cold = workspace.path(".forge/cold.sqlite");
        workspace.build_to(&cold);
        assert_eq!(doc_edges(&workspace.db()), doc_edges(&cold));
    }
    assert_eq!(doc_edges(&workspace.db()), baseline);
}

#[test]
fn delta_and_cold_roots_match_after_document_rowid_changes() {
    let workspace = Workspace::new();
    workspace.write("docs/guide.md", "# Guide\nKeep the first token.\n");
    workspace.write("docs/zzz.md", "# Later\nKeep the final document rowid.\n");
    workspace.build();

    workspace.write("docs/guide.md", "# Guide\nReplace the first token.\n");
    let delta = workspace.delta(&["docs/guide.md"]);
    let cold = workspace.build_to(workspace.path(".forge/cold.sqlite"));

    assert_eq!(
        delta["output_root"], cold["output_root"],
        "document FTS commitments must use stable document identities, not SQLite rowids"
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn source_delta_relinks_affected_docs_with_bare_suffix_references() {
    let workspace = Workspace::new();
    workspace.write("provider.py", "def target(): return 1\n");
    workspace.write(
        "settings.toml",
        "[tool.ruff]\ntarget-version = 'py311'\n[tool.AND]\nenabled = true\n",
    );
    workspace.write(
        "docs/guide.md",
        "# Guide\nUse `target` and configure `ruff` and `AND`.\n",
    );
    workspace.build();
    let baseline = doc_edges(&workspace.db());
    assert!(
        ["tool.ruff", "tool.AND"].iter().all(|name| {
            baseline.iter().any(|(src, dst, kind)| {
                src.starts_with("doc:")
                    && dst.contains("settings.toml")
                    && dst.ends_with(name)
                    && kind == "documents"
            })
        }),
        "cold graph must link bare names, including an FTS operator, to TOML namespaces: {baseline:?}"
    );

    for source in [
        "def target(): return 1\ndef marker(): pass\n",
        "def target(): return 1\n",
    ] {
        workspace.write("provider.py", source);
        let report = workspace.delta(&["provider.py"]);
        assert!(report["affected_owners"].as_u64().unwrap() > 1, "{report}");
        let cold = workspace.path(".forge/cold.sqlite");
        workspace.build_to(&cold);
        assert_eq!(doc_edges(&workspace.db()), doc_edges(&cold));
    }
    assert_eq!(doc_edges(&workspace.db()), baseline);

    // Documentary claims are checked against the current source snapshot, not mtimes.
    let original_doc = "# Guide\nUse `target` and configure `ruff` and `AND`.\n- INV-STABLE: Preserve [`target`](../provider.py#L1).\n[Other](other.md#other)\n";
    workspace.write("docs/guide.md", original_doc);
    workspace.write("docs/other.md", "# Other\n[Guide](guide.md#guide)\n");
    workspace.build();
    let linked = doc_edges(&workspace.db());
    assert!(linked.iter().any(|(_, _, kind)| kind == "cites_doc"));
    assert!(linked.iter().any(|(_, _, kind)| kind == "governs"));
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(100), 0, None, None).unwrap();
    for (source, document, other, stale_count, dead_count, broken_count) in [
        ("# shifted\n# valid line claim\ndef target(): return 1\ndef marker(): pass\n", original_doc, "# Other\n[Guide](guide.md#guide)\n", 1, 0, 0),
        ("# shifted\n# valid line claim\ndef target(): return 1\ndef marker(): pass\n", "# Guide\nUse `target` and configure `ruff` and `AND`.\n- INV-STABLE: Preserve [`target`](../provider.py#L3).\n[Other](other.md#other)\n", "# Other\n[Guide](guide.md#guide)\n", 0, 0, 0),
        ("# shifted\n# valid line claim\ndef target(): return 1\ndef marker(): pass\n", "# Guide\nUse `target` and configure `ruff` and `AND`.\n- INV-STABLE: Preserve [`target`](../provider.py#L2-L4).\n[Other](other.md#other)\n", "# Other\n[Guide](guide.md#guide)\n", 0, 0, 0),
        ("def target_renamed(): return 1\n", original_doc, "# Other\n[Guide](guide.md#guide)\n", 0, 1, 0),
        ("def target(): return 1\n", original_doc, "# Changed heading\n[Guide](guide.md#guide)\n", 0, 0, 1),
        ("def target(): return 1\n", original_doc, "# Other\n[Guide](guide.md#guide)\n", 0, 0, 0),
    ] {
        workspace.write("provider.py", source);
        workspace.write("docs/guide.md", document);
        workspace.write("docs/other.md", other);
        let delta = workspace.delta(&["provider.py", "docs/guide.md", "docs/other.md"]);
        let cold_path = workspace.path(".forge/cold.sqlite");
        let cold = workspace.build_to(&cold_path);
        assert_eq!(delta["output_root"], cold["output_root"], "current cold/delta facts must agree: {source} / {document}");
        assert_eq!(doc_edges(&workspace.db()), doc_edges(&cold_path));
        let live = reader::open(&workspace.db(), workspace.root()).unwrap();
        let cold = reader::open(&cold_path, workspace.root()).unwrap();
        let document = reader::get_doc_paged(&live, "docs/guide.md", None, &options).unwrap();
        assert_eq!(document, reader::get_doc_paged(&cold, "docs/guide.md", None, &options).unwrap());
        assert_eq!(document["cites_doc"]["total"], if broken_count == 0 { 1 } else { 0 });
        let analysis = reader::analyze_paged(&live, "docs/guide.md", Some(false), &options).unwrap();
        assert_eq!(analysis, reader::analyze_paged(&cold, "docs/guide.md", Some(false), &options).unwrap());
        let diagnostics = analysis["doc_diagnostics"]["items"].as_array().expect("documentary source diagnostics");
        for (kind, expected) in [("stale_doc_sections", stale_count), ("dead_doc_symbol", dead_count), ("broken_doc_link", broken_count)] {
            let count = diagnostics.iter().filter(|item| item["kind"] == kind).count();
            if kind == "dead_doc_symbol" && expected > 0 {
                assert!(count >= expected, "dead references must be diagnosed: {analysis}");
            } else {
                assert_eq!(count, expected, "{kind}: {analysis}");
            }
        }
        for stale in diagnostics.iter().filter(|item| item["kind"] == "stale_doc_sections") {
            assert_eq!(stale["reason"], "documented_symbol_range_mismatch");
            assert_eq!(stale["claim"]["target_path"], "provider.py");
            assert_eq!(stale["claim"]["start_line"], 1);
            assert_eq!(stale["target"]["line"], 3);
        }
        if dead_count > 0 {
            assert!(!document["referenced_symbols"]["items"].as_array().unwrap().iter()
                .any(|symbol| symbol["name"] == "target_renamed"));
        }
    }
    assert_eq!(doc_edges(&workspace.db()), linked);

    let identity_workspace = Workspace::new();
    let linked = Workspace::new();
    identity_workspace.write("forge-mcp.yaml", &format!("roots: [.]\ndocs: [docs]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [src, frontend]\n    docs: [docs]\n", linked.root().display()));
    identity_workspace.write(
        "primary.py",
        "def validate(): return 1\ndef primary_only(): return 2\n",
    );
    identity_workspace.write(
        "docs/context-primary.md",
        "# Primary\nUse `validate` and `linked_only`.\n",
    );
    identity_workspace.write(
        "docs/cross-workspace.md",
        "# Cross workspace\n[`validate`](../[library]/src/validate.py)\n",
    );
    linked.write("src/validate.py", "# validation module\n");
    linked.write("src/validate/__init__.py", "# validation package\n");
    linked.write("src/validate/validate.py", "# nested validation module\n");
    let header_source =
        "class GridViewHeader:\n    content: str = ''\ndef linked_only(): return 3\n";
    linked.write("src/header.py", header_source);
    linked.write(
        "frontend/header.ts",
        "export interface GridViewHeader {\n  content: string;\n}\n",
    );
    linked.write(
        "docs/context-linked.md",
        "# Linked\nUse `validate` and `primary_only`.\n",
    );
    linked.write("docs/header.md", "# Header\nUse `GridViewHeader.content` and `GridViewHeader.content`.\n## Other\nUse `GridViewHeader.content`.\n");
    identity_workspace.write("provider.py", "def shared_target(): return 1\n");
    identity_workspace.write("other.py", "def shared_target(): return 2\n");
    identity_workspace.write("qualified.py", "class package:\n    def live(): return 1\nclass extra:\n    class package:\n        def live(): return 2\nclass suffix_only:\n    def leaf(): return 3\n");
    identity_workspace.write("src/main.rs", "pub fn target() {}\n");
    identity_workspace.write("src/provider.rs", "pub fn target() {}\n");
    identity_workspace.write("target.rs", "pub fn marker() {}\n");
    identity_workspace.write("src/unique.rs", "pub fn unique() {}\n");
    identity_workspace.write("src/qualified.rs", "pub mod inner {\n    pub fn live() {}\n}\npub mod outer {\n    pub mod src {\n        pub mod qualified {\n            pub mod inner {\n                pub fn live() {}\n            }\n        }\n    }\n}\npub mod suffix_only {\n    pub fn leaf() {}\n}\n");
    identity_workspace.write(
        "claims.py",
        "# range has no declaration\ndef alpha(): return 1\ndef beta(): return 2\n",
    );
    let exact_doc =
        "# Exact file\n[`shared_target`](../provider.py)\n[`shared_target`](../provider.py#L1)\n";
    identity_workspace.write("docs/exact-file.md", exact_doc);
    identity_workspace.write(
        "docs/missing-file.md",
        "# Missing file\n[`unique`](../absent.py#L1)\n",
    );
    identity_workspace.write(
        "docs/unique-file.md",
        "# Exact unique\n[`unique`](../src/unique.rs)\n",
    );
    identity_workspace.write(
        "docs/unlinked-unique.md",
        "# Unlinked unique\nUse `unique` in this sentence.\n",
    );
    identity_workspace.write(
        "docs/ambiguous-unlinked.md",
        "# Ambiguous unlinked\nUse `shared_target` without a file selector.\n",
    );
    identity_workspace.write(
        "docs/mixed-occurrences.md",
        "# Mixed occurrences\n[`unique`](../src/unique.rs)\n[`unique`](../src/absent.rs#L1)\nUse `unique` here too.\n",
    );
    identity_workspace.write(
        "docs/module-identity.md",
        "# Module identity\n[`src.unique`](../src/unique.rs)\n",
    );
    identity_workspace.write(
        "docs/path-tier.md",
        "# Path tier\n[`target`](../src/provider.rs#L1)\n",
    );
    identity_workspace.write(
        "docs/unsafe-file.md",
        "# Unsafe path\n[`unique`](../../outside.rs#L1)\n",
    );
    identity_workspace.write(
        "docs/dot-qualified.md",
        "# Qualified\n[`qualified.package.live`](../qualified.py#L2)\n",
    );
    identity_workspace.write(
        "docs/colon-qualified.md",
        "# Colon qualified\n[`qualified::package::live`](../qualified.py#L2)\n",
    );
    identity_workspace.write(
        "docs/qualified-priority.md",
        "# Qualified priority\n[`src.qualified.inner.live`](../src/qualified.rs#L1)\n[`src::qualified::inner::live`](../src/qualified.rs#L1)\n",
    );
    identity_workspace.write(
        "docs/suffix-only.md",
        "# Suffix only\n[`suffix_only.leaf`](../src/qualified.rs)\n[`suffix_only::leaf`](../src/qualified.rs)\n",
    );
    identity_workspace.write(
        "docs/multi-claim.md",
        "# Multi claim\n[`alpha` and `beta`](../claims.py#L1)\n",
    );
    identity_workspace.write(
        "docs/separate-claims.md",
        "# Separate claims\n[`alpha`](../claims.py#L1) [`beta`](../claims.py#L1)\n",
    );
    identity_workspace.build();
    let mut live = reader::open(&identity_workspace.db(), identity_workspace.root()).unwrap();
    let mut regressions = Vec::new();
    let exact = reader::get_doc_paged(&live, "docs/exact-file.md", None, &options).unwrap();
    if !exact["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "provider.py" && symbol["name"] == "shared_target")
    {
        regressions.push(format!(
            "explicit file claim failed to resolve despite a same-name symbol elsewhere: {exact}"
        ));
    }
    let missing = reader::get_doc_paged(&live, "docs/missing-file.md", None, &options).unwrap();
    if missing["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "unique" && symbol["kind"] == "function")
    {
        regressions.push(format!(
            "explicit missing file promoted a symbol from another file: {missing}"
        ));
    }
    let unique = reader::get_doc_paged(&live, "docs/unique-file.md", None, &options).unwrap();
    if !unique["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| {
            symbol["path"] == "src/unique.rs"
                && symbol["name"] == "unique"
                && symbol["kind"] == "function"
        })
    {
        regressions.push(format!(
            "explicit same-file declaration lost to the containing module suffix: {unique}"
        ));
    }
    let unlinked_unique =
        reader::get_doc_paged(&live, "docs/unlinked-unique.md", None, &options).unwrap();
    if !unlinked_unique["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/unique.rs" && symbol["name"] == "unique")
    {
        regressions.push(format!(
            "unlinked exact declared-name occurrence lost to the module suffix: {unlinked_unique}"
        ));
    }
    let ambiguous_unlinked =
        reader::get_doc_paged(&live, "docs/ambiguous-unlinked.md", None, &options).unwrap();
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
    let mixed_occurrences =
        reader::get_doc_paged(&live, "docs/mixed-occurrences.md", None, &options).unwrap();
    let conn = Connection::open(identity_workspace.db()).unwrap();
    let mut statement = conn
        .prepare("SELECT e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.kind='documents' AND p.path='docs/mixed-occurrences.md' ORDER BY e.line")
        .unwrap();
    let mixed_lines = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    if !mixed_occurrences["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/unique.rs" && symbol["name"] == "unique")
        || mixed_lines != [2]
    {
        regressions.push(format!(
            "a missing same-token link must not erase or authorize another path; edges store one row per document-section/symbol, so line 2 is the valid canonical source line, got {mixed_lines:?}: {mixed_occurrences}"
        ));
    }
    let module = reader::get_doc_paged(&live, "docs/module-identity.md", None, &options).unwrap();
    if !module["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| {
            symbol["path"] == "src/unique.rs"
                && symbol["name"] == "unique.rs"
                && symbol["kind"] == "module"
        })
    {
        regressions.push(format!(
            "exact module identity was replaced by the same-basename declaration: {module}"
        ));
    }
    let path_tier = reader::get_doc_paged(&live, "docs/path-tier.md", None, &options).unwrap();
    if !path_tier["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/provider.rs" && symbol["name"] == "target")
    {
        regressions.push(format!(
            "global exact qualified identity suppressed an eligible path-local suffix: {path_tier}"
        ));
    }
    let unsafe_path = reader::get_doc_paged(&live, "docs/unsafe-file.md", None, &options).unwrap();
    if unsafe_path["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/unique.rs" && symbol["name"] == "unique")
    {
        regressions.push(format!(
            "unsafe explicit path fell back to a workspace declaration: {unsafe_path}"
        ));
    }
    let qualified = reader::get_doc_paged(&live, "docs/dot-qualified.md", None, &options).unwrap();
    if !qualified["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "qualified.py" && symbol["name"] == "live")
    {
        regressions.push(format!(
            "dot-qualified name failed to resolve to its indexed declaration: {qualified}"
        ));
    }
    let colon_qualified =
        reader::get_doc_paged(&live, "docs/colon-qualified.md", None, &options).unwrap();
    if !colon_qualified["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "qualified.py" && symbol["name"] == "live")
    {
        regressions.push(format!(
            "legacy :: qualified identity no longer resolves: {colon_qualified}"
        ));
    }
    let qualified_priority_refs =
        reader::get_doc_paged(&live, "docs/qualified-priority.md", None, &options).unwrap();
    if !qualified_priority_refs["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/qualified.rs" && symbol["name"] == "live")
    {
        regressions.push(format!(
            "exact qualified declaration did not produce a verified reference: {qualified_priority_refs}"
        ));
    }
    let qualified_priority =
        reader::analyze_paged(&live, "docs/qualified-priority.md", Some(false), &options).unwrap();
    let priority_stale = qualified_priority["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .collect::<Vec<_>>();
    if priority_stale.len() != 2
        || priority_stale
            .iter()
            .any(|item| item["target"]["line"] != 2)
    {
        regressions.push(format!(
            "exact qualified target must beat deeper suffix for dot and :: spellings: refs={qualified_priority_refs}, diagnostics={qualified_priority}"
        ));
    }
    let suffix_only = reader::get_doc_paged(&live, "docs/suffix-only.md", None, &options).unwrap();
    if !suffix_only["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "src/qualified.rs" && symbol["name"] == "leaf")
    {
        regressions.push(format!(
            "legacy dotted/:: suffix references stopped resolving: {suffix_only}"
        ));
    }
    let multi = reader::analyze_paged(&live, "docs/multi-claim.md", Some(false), &options).unwrap();
    if multi["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "stale_doc_sections")
    {
        regressions.push(format!(
            "multi-symbol label proved a unique stale range: {multi}"
        ));
    }
    let separate =
        reader::analyze_paged(&live, "docs/separate-claims.md", Some(false), &options).unwrap();
    let separate_count = separate["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .count();
    if separate_count != 2 {
        regressions.push(format!("separate one-symbol links should be independently stale, got {separate_count}: {separate}"));
    }

    identity_workspace.write(
        "docs/exact-file.md",
        &format!("{exact_doc}\nA harmless delta keeps the same explicit source claim.\n"),
    );
    let delta = identity_workspace.delta(&["docs/exact-file.md"]);
    let cold_path = identity_workspace.path(".forge/cold-identities.sqlite");
    let cold_report = identity_workspace.build_to(&cold_path);
    assert_eq!(
        delta["output_root"], cold_report["output_root"],
        "document identity facts must match after a cold build"
    );
    assert_eq!(doc_edges(&identity_workspace.db()), doc_edges(&cold_path));
    live = reader::open(&identity_workspace.db(), identity_workspace.root()).unwrap();
    let cold = reader::open(&cold_path, identity_workspace.root()).unwrap();
    let exact = reader::get_doc_paged(&live, "docs/exact-file.md", None, &options).unwrap();
    if !exact["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "provider.py" && symbol["name"] == "shared_target")
    {
        regressions.push(format!(
            "after document delta, explicit file claim failed to resolve: {exact}"
        ));
    }
    let missing = reader::get_doc_paged(&live, "docs/missing-file.md", None, &options).unwrap();
    if missing["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "unique" && symbol["kind"] == "function")
    {
        regressions.push(format!(
            "after document delta, explicit missing file promoted another file: {missing}"
        ));
    }
    let qualified = reader::get_doc_paged(&live, "docs/dot-qualified.md", None, &options).unwrap();
    if !qualified["referenced_symbols"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["path"] == "qualified.py" && symbol["name"] == "live")
    {
        regressions.push(format!(
            "after document delta, dot-qualified name failed: {qualified}"
        ));
    }
    let multi = reader::analyze_paged(&live, "docs/multi-claim.md", Some(false), &options).unwrap();
    if multi["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "stale_doc_sections")
    {
        regressions.push(format!(
            "after document delta, multi-symbol label proved stale: {multi}"
        ));
    }
    let separate =
        reader::analyze_paged(&live, "docs/separate-claims.md", Some(false), &options).unwrap();
    let separate_count = separate["doc_diagnostics"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "stale_doc_sections")
        .count();
    if separate_count != 2 {
        regressions.push(format!(
            "after document delta, separate one-symbol links lost stale evidence: {separate}"
        ));
    }
    for path in [
        "docs/exact-file.md",
        "docs/missing-file.md",
        "docs/dot-qualified.md",
        "docs/colon-qualified.md",
        "docs/unique-file.md",
        "docs/unlinked-unique.md",
        "docs/ambiguous-unlinked.md",
        "docs/mixed-occurrences.md",
        "docs/module-identity.md",
        "docs/path-tier.md",
        "docs/unsafe-file.md",
        "docs/qualified-priority.md",
        "docs/suffix-only.md",
        "docs/multi-claim.md",
        "docs/separate-claims.md",
    ] {
        let live_doc = reader::get_doc_paged(&live, path, None, &options).unwrap();
        assert_eq!(
            live_doc,
            reader::get_doc_paged(&cold, path, None, &options).unwrap(),
            "{path}"
        );
        let live_analysis = reader::analyze_paged(&live, path, Some(false), &options).unwrap();
        assert_eq!(
            live_analysis,
            reader::analyze_paged(&cold, path, Some(false), &options).unwrap(),
            "{path}"
        );
    }
    let full_options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(100),
        0,
        Some(contextunity_forge_mcp::core::response::Detail::Full),
        None,
    )
    .unwrap();
    for (path, verified_paths, candidate_paths) in [
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
        let document = reader::get_doc_paged(&live, path, None, &full_options).unwrap();
        let verified = document["referenced_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let candidates = document["referenced_symbol_candidates"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let candidate_targets = candidates
            .iter()
            .filter_map(|item| item["target"]["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if verified != verified_paths.into_iter().collect()
            || candidate_targets != candidate_paths.into_iter().collect()
            || document["referenced_symbol_candidates"]["total"].as_u64()
                != Some(candidates.len() as u64)
        {
            regressions.push(format!("document workspace/path eligibility must precede name tiers for {path}: {document}"));
        }
    }
    let unchanged_document = std::fs::read(linked.path("docs/header.md")).unwrap();
    for (source, expected_verified, expected_candidates) in [
        (Some(header_source), 0, 2),
        (
            Some("class GridViewHeader:\n    renamed: str = ''\ndef linked_only(): return 3\n"),
            1,
            0,
        ),
        (None, 1, 0),
        (Some(header_source), 0, 2),
    ] {
        match source {
            Some(source) => linked.write("src/header.py", source),
            None => std::fs::remove_file(linked.path("src/header.py")).unwrap(),
        }
        let report = identity_workspace.delta(&["[library]/src/header.py"]);
        let cold_path = identity_workspace.path(".forge/cold-context.sqlite");
        let cold_report = identity_workspace.build_to(&cold_path);
        assert_eq!(
            report["output_root"], cold_report["output_root"],
            "source-only linked edit/delete/restore must preserve complete cold/delta commitments"
        );
        assert_eq!(doc_edges(&identity_workspace.db()), doc_edges(&cold_path));
        assert_eq!(std::fs::read(linked.path("docs/header.md")).unwrap(), unchanged_document,
            "candidate transitions must follow source-only changes, without rewriting the documentary claim");
        let delta_connection =
            reader::open(&identity_workspace.db(), identity_workspace.root()).unwrap();
        let cold_connection = reader::open(&cold_path, identity_workspace.root()).unwrap();
        for path in [
            "docs/context-primary.md",
            "docs/cross-workspace.md",
            "[library]/docs/context-linked.md",
            "[library]/docs/header.md",
        ] {
            let delta_document =
                reader::get_doc_paged(&delta_connection, path, None, &full_options).unwrap();
            let cold_document =
                reader::get_doc_paged(&cold_connection, path, None, &full_options).unwrap();
            assert_eq!(
                delta_document, cold_document,
                "full public readback must match after source-only transition for {path}"
            );
        }
        let document = reader::get_doc_paged(
            &delta_connection,
            "[library]/docs/header.md",
            Some("Header"),
            &full_options,
        )
        .unwrap();
        let candidates = document["referenced_symbol_candidates"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let candidate_paths = candidates
            .iter()
            .filter_map(|item| item["target"]["path"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if document["referenced_symbols"]["total"] != expected_verified
            || document["referenced_symbol_candidates"]["total"] != expected_candidates
            || candidates.len() != expected_candidates
            || candidates.iter().any(|item| {
                item["source"]["path"] != "[library]/docs/header.md"
                    || item["source"]["name"] != "GridViewHeader.content"
                    || item["source"]["line"] != 2
                    || !item["source"]["doc_id"].is_string()
                    || !item["target"]["id"].is_string()
                    || item["target"]["name"] != "content"
            })
            || (expected_candidates == 2
                && candidate_paths
                    != ["[library]/frontend/header.ts", "[library]/src/header.py"]
                        .into_iter()
                        .collect())
        {
            regressions.push(format!("source-only transition must distinguish a verified singleton from complete ambiguous discovery ({expected_verified}/{expected_candidates}): {document}"));
        }
        if expected_verified == 1
            && !document["referenced_symbols"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| {
                    item["path"] == "[library]/frontend/header.ts" && item["name"] == "content"
                })
        {
            regressions.push(format!("only the remaining live TS field may verify after Python edit/deletion: {document}"));
        }
        let forbidden_edges: i64 = delta_connection.query_row(
            "SELECT count(*) FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE p.path='[library]/docs/header.md' AND e.kind IN('documents','governs')",
            [], |row| row.get(0),
        ).unwrap();
        if expected_candidates == 2 && forbidden_edges != 0 {
            regressions.push(format!(
                "ambiguous discovery cannot create verified/governing edges: {forbidden_edges}"
            ));
        }
    }
    assert!(
        regressions.is_empty(),
        "Task5 incremental documentary regressions: {regressions:#?}"
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn readme_only_delta_preserves_verified_code_links_without_component_claims() {
    let workspace = Workspace::new();
    workspace.write("pkg/simple.py", "def simple(): return 1\n");
    workspace.write(
        "README.md",
        "# Package\nSee `pkg` for details and [`simple`](pkg/simple.py#L1) for the implementation.\n",
    );
    workspace.build();
    let connection = workspace.open();
    let simple_id: String = connection
        .query_row(
            "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='pkg/simple.py' AND n.name='simple' AND n.kind='function'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let baseline = doc_edges(&workspace.db());
    for updated in [false, true] {
        if updated {
            workspace.write(
                "README.md",
                "# Package Updated\nSee `pkg` for details and [`simple`](pkg/simple.py#L1) for the implementation.\n",
            );
            let delta = workspace.delta(&["README.md"]);
            let cold = workspace.path(".forge/cold.sqlite");
            let cold_report = workspace.build_to(&cold);
            assert_eq!(delta["output_root"], cold_report["output_root"]);
            assert_eq!(doc_edges(&workspace.db()), doc_edges(&cold));
        }
        let edges = doc_edges(&workspace.db());
        assert_eq!(
            edges.len(),
            2,
            "one verified declaration requires reciprocal documentary edges: {edges:?}"
        );
        assert!(
            edges.iter().any(|(src, dst, kind)| {
                src.contains("README.md") && dst == &simple_id && kind == "documents"
            }),
            "the explicit file claim must document the actual simple declaration: {edges:?}"
        );
        assert!(
            edges.iter().any(|(src, dst, kind)| {
                src == &simple_id && dst.contains("README.md") && kind == "references_doc"
            }),
            "the actual declaration must retain its reciprocal README reference: {edges:?}"
        );
        assert!(
            edges
                .iter()
                .all(|(src, dst, _)| src != "component:pkg" && dst != "component:pkg"),
            "a bare component token cannot establish a verified code claim: {edges:?}"
        );
        assert_eq!(
            edges, baseline,
            "a README heading edit must preserve all verified documentary edges"
        );
    }
}
