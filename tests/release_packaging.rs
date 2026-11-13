#![cfg(unix)]

#[allow(dead_code)]
mod common;

use anyhow::Context;
use common::Workspace;
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn workflow(name: &str) -> anyhow::Result<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".github/workflows")
        .join(name);
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read production workflow {}", path.display()))?;
    serde_yaml::from_str(&source).context("production workflow is valid YAML")
}

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_sequence().map(Vec::as_slice).unwrap_or(&[])
}

#[test]
fn ci_checks_pull_requests_and_main_on_linux_and_macos() {
    let ci = workflow("ci.yml");
    assert!(
        ci.is_ok(),
        "assertion failed: CI workflow is required: {ci:?}"
    );
    let ci = ci.unwrap();
    assert!(ci["on"].get("pull_request").is_some());
    assert!(ci["on"]["push"]["branches"]
        .as_sequence()
        .is_some_and(|branches| branches
            .iter()
            .any(|branch| branch.as_str() == Some("main"))));
    let jobs = ci["jobs"].as_mapping().expect("CI jobs");
    let runners = jobs
        .values()
        .filter_map(|job| job["strategy"]["matrix"]["os"].as_sequence())
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(runners.iter().any(|runner| runner.starts_with("ubuntu-")));
    assert!(runners.iter().any(|runner| runner.starts_with("macos-")));
    let commands = jobs
        .values()
        .flat_map(steps)
        .filter_map(|step| step["run"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for command in [
        "cargo check",
        "cargo clippy --workspace --all-targets --all-features -- -D warnings",
        "cargo test",
    ] {
        assert!(commands.contains(command), "CI must execute {command}");
    }
}

#[test]
fn release_declares_tag_and_nightly_builds_for_all_installer_targets() {
    let release = workflow("release.yml").or_else(|_| workflow("ci.yml"));
    assert!(
        release.is_ok(),
        "assertion failed: release workflow is required: {release:?}"
    );
    let release = release.unwrap();
    assert!(
        release["on"]["push"]["tags"]
            .as_sequence()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("v*"))),
        "release workflow must trigger on v* tag pushes"
    );
    let triggers_nightly = release["on"].get("schedule").is_some()
        || release["on"].get("workflow_dispatch").is_some()
        || release["on"]["push"]["branches"]
            .as_sequence()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("main")));
    assert!(
        triggers_nightly,
        "release workflow must declare nightly schedule/dispatch or main trigger"
    );
    let jobs = release["jobs"].as_mapping().expect("release jobs");
    let targets = jobs
        .values()
        .filter_map(|job| job["strategy"]["matrix"]["include"].as_sequence())
        .flatten()
        .filter_map(|row| row["target"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        targets,
        BTreeSet::from([
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
        ])
    );
    assert!(
        jobs.values().any(|job| {
            job.get("needs").is_some()
                && steps(job).iter().any(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("gh release") && run.contains("nightly"))
                })
        }),
        "publication must depend on builds and distinguish rolling nightly"
    );
}

#[test]
fn release_package_step_produces_installable_archive_and_checksum() {
    let release = workflow("release.yml").or_else(|_| workflow("ci.yml"));
    assert!(
        release.is_ok(),
        "assertion failed: release workflow is required: {release:?}"
    );
    let release = release.unwrap();
    let package = release["jobs"]
        .as_mapping()
        .expect("release jobs")
        .values()
        .flat_map(steps)
        .find(|step| step["id"].as_str() == Some("package"))
        .and_then(|step| step["run"].as_str());
    assert!(
        package.is_some(),
        "release must expose its actual package step"
    );
    let workspace = Workspace::new();
    let target = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        host => panic!("unsupported packaging proof host: {host:?}"),
    };
    let binary = workspace.path(format!("target/{target}/release/contextunity-forge-mcp"));
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_contextunity-forge-mcp"), &binary).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut companions = Vec::new();
    for entry in walkdir::WalkDir::new(repo.join("skills")).follow_links(true) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let relative = entry.path().strip_prefix(repo).unwrap().to_path_buf();
            let bytes = fs::read(entry.path()).unwrap();
            workspace.write_bytes(&relative, &bytes);
            companions.push((relative, bytes));
        }
    }
    let output = Command::new("bash")
        .args(["-e", "-o", "pipefail", "-c", package.unwrap()])
        .env("TARGET", target)
        .current_dir(workspace.root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "package step failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive_name = format!("contextunity-forge-mcp-{target}.tar.gz");
    let archive = workspace.path(format!("dist/{archive_name}"));
    let bytes = fs::read(&archive).expect("package step creates public archive name");
    let digest = hex::encode(Sha256::digest(&bytes));
    let sums = fs::read_to_string(workspace.path("dist/SHA256SUMS")).expect("release checksums");
    assert!(sums
        .lines()
        .any(|line| line == format!("{digest}  {archive_name}")));
    let unpacked = workspace.path("unpacked");
    fs::create_dir(&unpacked).unwrap();
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpacked)
        .output()
        .unwrap();
    assert!(output.status.success(), "release archive extracts cleanly");
    let output = Command::new(unpacked.join("contextunity-forge-mcp"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success(), "archived executable runs");
    assert!(String::from_utf8_lossy(&output.stdout).contains("contextunity-forge-mcp"));
    for (relative, expected) in companions {
        assert_eq!(
            fs::read(unpacked.join(&relative)).unwrap(),
            expected,
            "companion asset {}",
            relative.display()
        );
    }
    assert!(unpacked.join("skills/install.sh").is_file());
    assert!(unpacked
        .join("skills/contextunity-forge/SKILL.md")
        .is_file());
}

#[test]
fn proto_c_compilation_selects_c11_without_compiler_warnings() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/forge-index/build.rs"),
    )
    .unwrap();
    assert!(
        source.contains(".std(\"c11\")"),
        "proto C build must select C11"
    );
    assert!(
        source.contains(".warnings(false)"),
        "proto C build disables C warnings"
    );
}
