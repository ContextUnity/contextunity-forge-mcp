use crate::languages::harness::ExtractorHarness;

#[test]
fn cargo_imported_constructors_authorize_external_members() {
    for (label, import, constructor, member, authorized) in [
        (
            "canonical",
            "use rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            true,
        ),
        (
            "alias",
            "use rusqlite::Connection as Db;",
            "Db::open_in_memory()",
            "execute",
            true,
        ),
        (
            "declared",
            "use rusqlite::Connection;",
            "Connection::open_with_flags(\"db\", flags)",
            "execute",
            true,
        ),
        (
            "homonym",
            "use unregistered_crate::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "suffix",
            "use unregistered_crate::rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "undeclared",
            "use rusqlite::Connection;",
            "Connection::with_capacity(12)",
            "execute",
            false,
        ),
        (
            "std-file-result",
            "use std::fs::File;",
            "File::open(\"input\")",
            "metadata",
            true,
        ),
        (
            "builtin-string-result",
            "use std::str::FromStr;",
            "String::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "imported-string-result",
            "use std::string::String as Text; use std::str::FromStr;",
            "Text::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "tempfile-named",
            "use tempfile::NamedTempFile;",
            "NamedTempFile::new()",
            "path",
            true,
        ),
        (
            "clap-command",
            "use clap::Command;",
            "Command::new(\"app\")",
            "get_matches",
            true,
        ),
        (
            "clap-command-missing-unwrap",
            "use clap::Command;",
            "Command::new(\"app\")",
            "get_matches",
            false,
        ),
        (
            "regex-match",
            "use regex::Regex;",
            "Regex::new(\"pattern\")",
            "is_match",
            true,
        ),
    ] {
        let projection = if label == "clap-command" {
            ""
        } else {
            ".unwrap()"
        };
        let source = format!(
            "{import}\nfn run() {{ let conn = {constructor}{projection}; conn.{member}(); }}"
        );
        for path in ["identity.rs", "src/identity.rs"] {
            let graph = ExtractorHarness::for_lang("rust")
                .at_path(path)
                .linked(&source);
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == format!("conn.{member}"))
                .unwrap();
            assert_eq!(
                coverage.status == "external",
                authorized,
                "{label} at {path}: {coverage:#?}"
            );
            if authorized {
                assert_eq!(
                    coverage.status, "external",
                    "{label} at {path}: {coverage:#?}"
                );
            }
        }
    }
    for (label, source, expected_expr) in [
        (
            "duplicate-rust-alias",
            "use regex::Regex as Re; use unregistered_crate::Other as Re; fn run() { let r = Re::new(\"x\").unwrap(); r.is_match(\"x\"); }",
            "r.is_match",
        ),
        (
            "local-missing-type",
            "mod regex {} fn run(r: regex::Missing) { r.is_match(\"x\"); }",
            "r.is_match",
        ),
    ] {
        for path in ["negative.rs", "src/negative.rs"] {
            let graph = ExtractorHarness::for_lang("rust")
                .at_path(path)
                .linked(source);
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == expected_expr);
            assert!(
                coverage.is_none() || coverage.unwrap().status == "unresolved",
                "{label} at {path}: expected unresolved, got {coverage:#?}"
            );
        }
    }
}

#[test]
fn cargo_ecosystem_receivers_preserve_exact_provider_identity() {
    let cargo_source = r#"
use rusqlite::Connection;
use tree_sitter::Parser;

fn run_cargo_crates() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("SELECT 1", []).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
}
"#;
    let graph = ExtractorHarness::for_lang("rust")
        .at_path("cargo_test.rs")
        .linked(cargo_source);
    let execute_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "conn.execute")
        .expect("missing coverage for conn.execute");
    assert_eq!(
        execute_cov.status, "external",
        "conn.execute must resolve to external via rust-cargo.toml declarative pack: {execute_cov:#?}"
    );
    assert!(
        execute_cov.evidence.contains("rusqlite::Connection")
            && execute_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "conn.execute must retain its resolved import origin: {execute_cov:#?}"
    );
    let parser_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "parser.set_language")
        .expect("missing coverage for parser.set_language");
    assert_eq!(
        parser_cov.status, "external",
        "parser.set_language must resolve to external via rust-cargo.toml declarative pack: {parser_cov:#?}"
    );
    assert!(
        parser_cov.evidence.contains("tree_sitter::Parser")
            && parser_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "parser.set_language must retain its resolved import origin: {parser_cov:#?}"
    );
}

#[test]
fn string_does_not_inherit_iterator_members() {
    let iso_source = r#"
fn check_string_trait_isolation() {
    let s = String::new();
    s.clone();
    s.next();
}
"#;
    let graph = ExtractorHarness::for_lang("rust")
        .at_path("iso_test.rs")
        .linked(iso_source);
    let next_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "s.next")
        .expect("missing coverage for s.next");
    assert_eq!(
        next_cov.status, "unresolved",
        "s.next must remain unresolved because String does not implement Iterator: {next_cov:#?}"
    );
}

#[test]
fn git2_walkdir_and_chrono_packs_resolve_external_receivers() {
    let eco_source = r#"
use git2::Repository;
use walkdir::WalkDir;
use chrono::Utc;

fn test_ecosystem_packs() {
    let repo = Repository::open(".").unwrap();
    let _head = repo.head();
    let head = repo.head().unwrap();
    let commit = head.peel_to_commit().unwrap();
    let tree = commit.tree().unwrap();
    let index = repo.index().unwrap();
    tree.len();
    index.len();
    let now = Utc::now();
    now.timestamp();
    for entry in WalkDir::new(".").into_iter().filter_map(|e| e.ok()) {
        let _p = entry.path();
        entry.file_name();
        entry.file_type();
        entry.depth();
    }
}
"#;
    let graph = ExtractorHarness::for_lang("rust")
        .at_path("ecosystem_test.rs")
        .linked(eco_source);
    let head_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "repo.head")
        .expect("missing coverage for repo.head");
    assert_eq!(
        head_cov.status, "external",
        "repo.head must resolve to external via git2 ecosystem pack: {head_cov:#?}"
    );
    let entry_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.path")
        .expect("missing coverage for entry.path");
    assert_eq!(
        entry_cov.status, "external",
        "entry.path must resolve to external via walkdir DirEntry: {entry_cov:#?}"
    );
    for expression in [
        "head.peel_to_commit",
        "commit.tree",
        "tree.len",
        "index.len",
        "now.timestamp",
        "entry.file_name",
        "entry.file_type",
        "entry.depth",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "external", "{coverage:#?}");
    }
}

fn rayon_source(import: &str) -> String {
    format!("{import} struct Item; impl Item {{ fn finish(&self) {{}} }} struct Fake; impl Fake {{ fn par_iter(&self) -> Fake {{ Fake }} }} fn probe(mut items: Vec<Item>, slice: &[Item], fake: Fake) {{ let constructed = Vec::from([1]); constructed.par_iter().count(); let stream = items.par_iter(); stream.map(|item| item).count(); items.par_iter_mut().filter(|_| true).count(); items.into_par_iter().map(|_| 1).count(); slice.par_iter().map(|item| item).count(); fake.par_iter().count(); let changed = slice.par_iter().map(|_| 1).collect(); for item in changed {{ item.finish(); }} }}")
}

#[test]
fn rayon_authority_follows_dependency_prelude_and_module_scope() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (case, dependency, prelude, manifest, resolved) in [
        ("enabled", true, true, None, true),
        ("absent-dependency", false, true, None, false),
        ("absent-prelude", true, false, None, false),
        ("disabled", true, true, Some("enabled = false\n"), false),
        (
            "replaced",
            true,
            true,
            Some("enabled = true\nreplace = true\n"),
            false,
        ),
        ("no-return", true, true, Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['Vec', 'Slice']\nprovider = 'rayon'\nmembers = ['par_iter', 'par_iter_mut', 'into_par_iter']\n"), false),
    ] {
        workspace.write(
            "Cargo.toml",
            if dependency {
                "[package]\nname = 'rayon-probe'\nversion = '0.1.0'\n[dependencies]\nrayon = '1'\n"
            } else {
                "[package]\nname = 'rayon-probe'\nversion = '0.1.0'\n"
            },
        );
        if let Some(manifest) = manifest {
            workspace.write(".forge/frameworks/rust-cargo.toml", manifest);
        } else {
            let _ = std::fs::remove_file(workspace.path(".forge/frameworks/rust-cargo.toml"));
        }
        let import = if prelude {
            "use rayon::prelude::*;"
        } else {
            ""
        };
        let source = rayon_source(import);
        workspace.write("src/lib.rs", &source);
        let catalog = ExtractorHarness::linked_with_root(
            "rust",
            &[("src/lib.rs", source.as_str())],
            Some(workspace.root()),
        );
        let statuses: Vec<(String, String)> = catalog
            .graph
            .coverage
            .iter()
            .filter(|row| row.path == "src/lib.rs" && row.expression.ends_with(".count"))
            .map(|row| (row.expression.clone(), row.status.clone()))
            .collect();
        let entry = catalog.coverage_status("src/lib.rs", "items.par_iter");
        assert_eq!(
            entry,
            if resolved || case == "no-return" {
                "external"
            } else {
                "unresolved"
            },
            "{case}: extension entry authority"
        );
        assert_eq!(statuses.len(), 6, "{case}: {statuses:?}");
        for (expression, status) in statuses {
            let expected = if resolved && !expression.starts_with("fake.") {
                "external"
            } else {
                "unresolved"
            };
            assert_eq!(status, expected, "{case}: {expression}");
        }
        assert_eq!(
            catalog.coverage_status("src/lib.rs", "item.finish"),
            "unresolved",
            "{case}: unproven parallel callback result"
        );
    }
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = 'rayon-scope-probe'\nversion = '0.1.0'\n[dependencies]\nrayon = '1'\n",
    );
    for (case, import, expected) in [
        ("outer-only", "", "unresolved"),
        ("explicit-child", "use rayon::prelude::*;", "external"),
        ("use-super", "use super::*;", "external"),
    ] {
        for layout in ["inline", "file", "nested", "nested-bridge", "nested-lib"] {
            let child = format!("{import} pub fn probe(items: Vec<i32>, slice: &[i32]) {{ items.par_iter().count(); slice.par_iter().count(); }}");
            let mut sources: Vec<(String, String)> = Vec::new();
            if layout == "file" {
                sources.push((
                    "src/lib.rs".into(),
                    "use rayon::prelude::*; mod child;".into(),
                ));
                sources.push(("src/child.rs".into(), child));
            } else {
                let child_mod = if layout.starts_with("nested") {
                    let bridge = if layout == "nested-bridge" {
                        "use super::*;"
                    } else {
                        ""
                    };
                    let name = if layout == "nested-lib" {
                        "lib"
                    } else {
                        "child"
                    };
                    format!("mod middle {{ {bridge} mod {name} {{ {child} }} }}")
                } else {
                    format!("mod child {{ {child} }}")
                };
                sources.push((
                    "src/lib.rs".into(),
                    format!("use rayon::prelude::*; {child_mod}"),
                ));
            }
            for (path, source) in &sources {
                workspace.write(path, source);
            }
            let borrowed: Vec<(&str, &str)> = sources
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect();
            let catalog =
                ExtractorHarness::linked_with_root("rust", &borrowed, Some(workspace.root()));
            let statuses: Vec<(String, String)> = catalog
                .graph
                .coverage
                .iter()
                .filter(|row| {
                    row.expression.contains(".par_iter") || row.expression.ends_with(".count")
                })
                .map(|row| (row.expression.clone(), row.status.clone()))
                .collect();
            assert_eq!(statuses.len(), 4, "{case}, layout={layout}: {statuses:?}");
            for (expression, status) in statuses {
                let expected = if matches!(layout, "nested" | "nested-lib") && case == "use-super" {
                    "unresolved"
                } else {
                    expected
                };
                assert_eq!(status, expected, "{case}, layout={layout}: {expression}");
            }
        }
    }
}
