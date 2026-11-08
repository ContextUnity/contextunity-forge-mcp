#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::Workspace;
use std::process::{Command, Output};

fn check(workspace: &Workspace) -> Output {
    Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["docs", "check", "--root"])
        .arg(workspace.root())
        .output()
        .unwrap()
}

#[test]
fn valid_links_and_anchors_resolve() {
    let workspace = Workspace::new();
    workspace.write("README.md", "[guide](docs/guide.md#hello-world-1)\n[local](#readme)\n# Readme\n\n[ref]: docs/guide.md#custom\n");
    workspace.write("docs/guide.md", "# Hello <em>World</em>!\n# Hello World!\nSetext title\n------------\n## Named {#custom}\n[setext](#setext-title)\n");
    let output = check(&workspace);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "All Markdown links and anchors resolve."
    );
}

#[test]
fn missing_files_and_anchors_report_source_lines() {
    let workspace = Workspace::new();
    workspace.write("README.md", "# Home\n[missing](docs/no.md)\n[bad](docs/guide.md#absent)\n[local](#missing)\n\n[ref]: docs/no-ref.md\n");
    workspace.write("docs/guide.md", "# Present\n");
    let output = check(&workspace);
    assert_eq!(output.status.code(), Some(1));
    let errors = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "README.md:2: missing target",
        "README.md:3: missing anchor #absent",
        "README.md:4: missing anchor #missing",
        "README.md:6: missing target",
    ] {
        assert!(
            errors.contains(expected),
            "missing {expected:?} in {errors}"
        );
    }
}

#[test]
fn directories_root_paths_and_file_autolinks_resolve() {
    let workspace = Workspace::new();
    workspace.write("docs/guide.md", "# Guide\n");
    workspace.write("skills/example/references/item.txt", "item\n");
    workspace.write("README.md", &format!("[directory](docs/)\n[references](skills/example/references/)\n[root](/docs/guide.md#guide)\n<file://{}>\n[localhost](file://localhost{})\n<https://example.com/>\n", workspace.root().join("docs/guide.md").display(), workspace.root().join("docs/guide.md").display()));
    let output = check(&workspace);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    workspace.write("README.md", "<file:///definitely-not-a-real-forge-file>\n");
    assert!(!check(&workspace).status.success());
}

#[test]
fn uri_schemes_are_case_insensitive_and_external_links_are_not_files() {
    let workspace = Workspace::new();
    workspace.write("docs/guide.md", "# Guide\n");
    for scheme in [
        "http",
        "HTTPS",
        "HtTpS",
        "MAILTO",
        "tel",
        "data",
        "urn",
        "custom+v1",
    ] {
        workspace.write("README.md", &format!("[external]({scheme}:example)\n"));
        let output = check(&workspace);
        assert!(
            output.status.success(),
            "{scheme}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    workspace.write(
        "README.md",
        &format!(
            "[local](FiLe://{})\n",
            workspace.root().join("docs/guide.md").display()
        ),
    );
    assert!(check(&workspace).status.success());
    workspace.write(
        "README.md",
        "[missing](FILE:///definitely-not-a-real-forge-file)\n",
    );
    assert!(!check(&workspace).status.success());
    workspace.write("README.md", "[local](docs/a:b.md)\n");
    assert!(!check(&workspace).status.success());
}

#[test]
fn source_line_anchors_check_bounds() {
    let workspace = Workspace::new();
    workspace.write("src/example.rs", "one\ntwo\nthree\n");
    workspace.write("docs/example.md", "one\ntwo\n");
    workspace.write("README.md", "[line](src/example.rs#L1)\n[range](src/example.rs#L2-L3)\n[markdown](docs/example.md#L2)\n");
    assert!(check(&workspace).status.success());
    workspace.write(
        "README.md",
        "[out](src/example.rs#L4)\n[range](src/example.rs#L2-L4)\n[zero](src/example.rs#L0)\n",
    );
    let output = check(&workspace);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("missing")
            .count(),
        3
    );
}

#[test]
fn discovers_workspace_markdown_and_excludes_generated_directories() {
    let workspace = Workspace::new();
    for path in ["AGENTS.md", "skills/example/SKILL.md", "other/note.md"] {
        workspace.write(path, "[missing](missing.md)\n");
    }
    for path in [
        "target/note.md",
        ".git/note.md",
        ".worktrees/example/note.md",
    ] {
        workspace.write(path, "[missing](missing.md)\n");
    }
    let output = check(&workspace);
    let errors = String::from_utf8_lossy(&output.stderr);
    for path in ["AGENTS.md", "skills/example/SKILL.md", "other/note.md"] {
        assert!(errors.contains(path), "{errors}");
    }
    for path in [
        "target/note.md",
        ".git/note.md",
        ".worktrees/example/note.md",
    ] {
        assert!(!errors.contains(path), "{errors}");
    }
}

#[test]
fn json_output_reports_success_and_failure() {
    let workspace = Workspace::new();
    workspace.write("README.md", "# Home\n");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["docs", "check", "--format", "json", "--root"])
            .arg(workspace.root())
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["files_checked"], 1);
    workspace.write("README.md", "[bad](missing.md)\n");
    let output = run();
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert!(value["errors"].as_array().unwrap()[0]
        .as_str()
        .unwrap()
        .contains("missing.md"));

    // Missing root with --format json outputs structured JSON
    let missing_root_output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "docs",
            "check",
            "--format",
            "json",
            "--root",
            "/nonexistent_forge_test_dir_12345",
        ])
        .output()
        .unwrap();
    assert!(!missing_root_output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&missing_root_output.stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert!(!value["errors"].as_array().unwrap().is_empty());
}

#[test]
fn line_number_overflow_reports_error() {
    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "pub fn a() {}\n");
    workspace.write(
        "README.md",
        "[overflow](src/lib.rs#L18446744073709551616)\n",
    );
    let output = check(&workspace);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid line range"));
}

#[test]
fn combining_marks_are_preserved_in_heading_slugs() {
    let workspace = Workspace::new();
    workspace.write("README.md", "# Cafe\u{301}\n[heading](#cafe\u{301})\n");
    let output = check(&workspace);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
