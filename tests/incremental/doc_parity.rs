#![cfg(feature = "lang-toml")]

use rusqlite::Connection;
use std::path::Path;

use crate::common::Workspace;

fn doc_edges(db: &Path) -> Vec<(String, String, String)> {
    let conn = Connection::open(db).unwrap();
    let mut statement = conn
        .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind FROM edges WHERE kind IN ('documents','references_doc') ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind")
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
}
