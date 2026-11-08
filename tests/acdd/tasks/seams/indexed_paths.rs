use super::*;

#[test]
fn contract_seam_accepts_root_cargo_package_imports_from_integration_tests() {
    let root = prepare_indexed_boundary_workspace("pub fn route() -> usize { 1 }\n",
"",
"use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
"route",
None,
None,
"tests/connected.rs");
    let profile_path = root.root().join(".forge/acdd/profile.yaml");
    let profile_before = std::fs::read(&profile_path).unwrap();
    root.write(
        "tests/connected.rs",
        "use boundary_fixture::api::route;\n#[test]\nfn integration_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
    );
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    TaskHarness::claim_at(
        root.root(),
        "forge-mcp/m-current-boundary:routing",
        "contract",
        "contract-author",
    )
    .unwrap();
    submit_indexed_boundary(&root, "contract", "tests/connected.rs").unwrap();
    assert_eq!(
        tasks::store_for_task(root.root(), "forge-mcp/m-current-boundary:routing")
            .unwrap()
            .inspect("forge-mcp/m-current-boundary:routing")
            .unwrap()
            .stage,
        "build"
    );
    assert_eq!(std::fs::read(profile_path).unwrap(), profile_before);
}
#[test]
fn contract_seam_requires_current_indexed_paths_at_every_hop() {
    assert_safe_git_environment();
    let api = "pub fn route() -> usize { 1 }\n";
    let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\npub fn outer() -> usize { helper() }\n";
    let direct = "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n";
    let indirect = "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n";
    let mut failures = Vec::new();
    let mut fixtures: HashMap<(&str, String, &str), (Workspace, FixtureSnapshot)> = HashMap::new();
    for (case, seam, refresh, reject) in [
        ("fresh_direct", "src/seam.rs::test_connected", false, false),
        (
            "fresh_two_hops",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
        ("three_hops", "src/seam.rs::test_connected", false, false),
        ("four_hops", "src/seam.rs::test_connected", false, false),
        (
            "removed_direct_call",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        ("removed_direct_call_file", "src/seam.rs", false, true),
        (
            "disconnected_helper",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        ("deleted_helper", "src/seam.rs::test_connected", false, true),
        ("deleted_target", "src/seam.rs::test_connected", false, true),
        ("unindexed_new_test", "src/seam.rs", false, true),
        (
            "refreshed_disconnected",
            "src/seam.rs::test_connected",
            true,
            true,
        ),
        (
            "refreshed_connected",
            "src/seam.rs::test_connected",
            true,
            false,
        ),
        (
            "unrelated_file_change",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
        (
            "retargeted_import",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "retargeted_local_alias",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "target_becomes_test",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "line_shift_only",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
    ] {
        let local_alias = "use crate::api::route;\nuse crate::other::route as other_route;\n#[test]\nfn test_connected() { let handler = other_route; let handler = route; let actual = handler(); assert_eq!(actual, 1); }\n";
        let initial = if matches!(
            case,
            "fresh_two_hops" | "disconnected_helper" | "deleted_helper"
        ) {
            indirect.to_owned()
        } else if case == "retargeted_local_alias" {
            local_alias.to_owned()
        } else if matches!(case, "three_hops" | "four_hops") {
            let helper = if case == "four_hops" {
                "wrapper"
            } else {
                "outer"
            };
            direct
                .replace("api::route", &format!("caller::{helper}"))
                .replace("route()", &format!("{helper}()"))
        } else {
            direct.to_owned()
        };
        let caller_source = if case == "four_hops" {
            "use crate::api::route;\npub fn helper() -> usize { route() }\npub fn outer() -> usize { helper() }\npub fn wrapper() -> usize { outer() }\n"
        } else {
            caller
        };
        let fixture = fixtures
            .entry((caller_source, initial.clone(), seam))
            .or_insert_with(|| {
                let root = indexed_boundary_workspace(api, caller_source, &initial, "route", seam);
                let snapshot = FixtureSnapshot::capture(&root);
                (root, snapshot)
            });
        fixture.1.restore(&fixture.0);
        let root = &fixture.0;
        match case {
            "removed_direct_call" | "removed_direct_call_file" | "refreshed_disconnected" => root
                .write(
                    "src/seam.rs",
                    "#[test]\nfn test_connected() { assert_eq!(1, 2); }\n",
                ),
            "disconnected_helper" => root.write(
                "src/caller.rs",
                "pub fn helper() -> usize { 2 }\npub fn outer() -> usize { helper() }\n",
            ),
            "deleted_helper" => root.write("src/caller.rs", "pub fn outer() -> usize { 2 }\n"),
            "deleted_target" => root.write("src/api.rs", "pub fn replacement() -> usize { 1 }\n"),
            "unindexed_new_test" => root.write(
                "src/seam.rs",
                &format!(
                    "#[test]\nfn test_connected() {{ assert_eq!(1, 2); }}\n{}",
                    direct.replace("test_connected", "test_new")
                ),
            ),
            "refreshed_connected" => root.write(
                "src/seam.rs",
                &direct
                    .replace("assert_eq!", "assert_ne!")
                    .replace(", 1)", ", 2)"),
            ),
            "unrelated_file_change" => {
                root.write("src/unrelated.rs", "pub fn unrelated() -> usize { 2 }\n")
            }
            "retargeted_import" => {
                root.write(
                    "src/seam.rs",
                    &direct.replace("use crate::api::route;", "use crate::other::route;"),
                );
                root.write("src/other.rs", "pub fn route() -> usize { 2 }\n");
            }
            "retargeted_local_alias" => root.write(
                "src/seam.rs",
                &local_alias.replace("let handler = route", "let handler = other_route"),
            ),
            "target_becomes_test" => {
                root.write("src/api.rs", "#[test]\npub fn route() -> usize { 1 }\n")
            }
            "line_shift_only" => root.write("src/seam.rs", &format!("// shifted line\n{direct}")),
            _ => {}
        }
        if refresh {
            root.build_to(root.path(".forge/code-map.sqlite"));
        }
        let result = submit_indexed_boundary(root, "contract", seam);
        match (reject, result) {
            (true, Err(error)) => {
                let error = error.to_string();
                let expected = if case == "refreshed_disconnected" {
                    "TASK_SEAM_DISCONNECTED"
                } else {
                    "TASK_SEAM_STALE"
                };
                if !error.contains(expected) {
                    failures.push(format!("{case}: expected {expected}, got: {error}"));
                }
                for diagnostic in [
                    "Calls discovered:",
                    "Coverage statuses:",
                    "Reached targets:",
                    "Stop reason:",
                ] {
                    if !error.contains(diagnostic) {
                        failures.push(format!(
                            "{case}: missing '{diagnostic}' diagnostic: {error}"
                        ));
                    }
                }
                if expected == "TASK_SEAM_STALE" && !error.contains("contextunity-forge-mcp scan") {
                    failures.push(format!(
                        "{case}: stale diagnostic omits refresh action: {error}"
                    ));
                }
                assert_eq!(
                    tasks::store_for_task(root.root(), "forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .inspect("forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .stage,
                    "contract"
                );
            }
            (true, Ok(_)) => failures.push(format!(
                "{case}: accepted a seam without a current indexed production path"
            )),
            (false, Err(error)) => failures.push(format!(
                "{case}: valid current indexed seam rejected: {error}"
            )),
            (false, Ok(_)) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn contract_seam_accepts_test_path_helpers_and_rejects_stale_hops() {
    let api = "pub fn route() -> usize { 1 }\n";
    let test_helper_source = "use crate::api::route;\npub fn helper() -> usize { route() }\n";
    let mut failures = Vec::new();

    let root = prepare_indexed_boundary_workspace(
        api,
        "",
        "use crate::test_helpers::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n",
        "route",
        Some(test_helper_source),
        Some("helper"),
        "src/seam.rs::test_connected",
    );
    let task_id = "forge-mcp/m-current-boundary:routing";
    let baseline_task = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    assert_eq!(baseline_task.status, "ready");
    assert_eq!(baseline_task.stage, "contract");
    assert!(baseline_task.worker_id.is_none());
    let baseline_head = git_head(root.root());
    let snapshot = FixtureSnapshot::capture(&root);
    let baseline_database = std::fs::read(root.path(".forge/tasks.sqlite")).unwrap();
    TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
    match submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected") {
        Ok(_) => {}
        Err(error) => failures.push(format!("fresh test-marked helper path rejected: {error}")),
    }

    snapshot.restore(&root);
    assert_eq!(git_head(root.root()), baseline_head, "fixture HEAD ref");
    assert_eq!(
        std::fs::read(root.path(".forge/tasks.sqlite")).unwrap(),
        baseline_database,
        "task database should return to the shared seed"
    );
    let restored_task = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    assert_eq!(restored_task.status, baseline_task.status);
    assert_eq!(restored_task.stage, baseline_task.stage);
    assert_eq!(restored_task.claim_revision, baseline_task.claim_revision);
    assert_eq!(restored_task.worker_id, baseline_task.worker_id);
    TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
    root.write(
        "src/test_helpers.rs",
        &test_helper_source.replace(
            "pub fn helper() -> usize { route() }",
            "fn helper() -> usize { 1 }",
        ),
    );
    match submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected") {
        Err(error) if error.to_string().contains("TASK_SEAM_STALE") => {
            let error = error.to_string();
            assert!(error.contains("contextunity-forge-mcp scan"), "{error}");
            assert!(error.contains("Reached targets:"), "{error}");
        }
        Err(error) => failures.push(format!(
            "stale helper failed at the wrong boundary: {error}"
        )),
        Ok(_) => {
            failures.push("stale test-marked helper path was accepted without reindexing".into())
        }
    }
    let stale_stage = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap()
        .stage;
    if stale_stage != "contract" {
        failures.push(format!(
            "stale helper rejection advanced task stage to {stale_stage}"
        ));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn contract_seam_validates_claimed_worktree_against_shared_primary_index() {
    let api = "pub fn route() -> usize { 1 }\n";
    let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\n";
    let indirect = "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n";
    let root = prepare_indexed_boundary_workspace(
        api,
        caller,
        indirect,
        "route",
        None,
        None,
        "src/seam.rs::test_connected",
    );
    let mut failures = Vec::new();
    let worktree = root.root().join("claimed-worktree");
    let added = TaskHarness::create_worktree(root.root(), &worktree, "claimed-boundary");
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    TaskHarness::claim_in_worktree(
        root.root(),
        "forge-mcp/m-current-boundary:routing",
        "contract",
        "contract-author",
        &worktree,
        Some(&TaskHarness::session_for("contract-author")),
    )
    .unwrap();
    for (case, stale_file) in [
        ("stale_test", "src/seam.rs"),
        ("stale_helper", "src/caller.rs"),
    ] {
        if stale_file == "src/caller.rs" {
            std::fs::write(worktree.join("src/seam.rs"), indirect).unwrap();
            std::fs::write(
                worktree.join(stale_file),
                "pub fn helper() -> usize { 2 }\n",
            )
            .unwrap();
        } else {
            std::fs::write(
                worktree.join(stale_file),
                "#[test]\nfn test_connected() { assert_eq!(1, 2); }\n",
            )
            .unwrap();
        }
        contextunity_forge_mcp::engine::pipeline::build_with_cache_dir(
            &worktree,
            &worktree.join(".forge/code-map.sqlite"),
            None,
            &worktree.join(".forge/extraction-cache"),
        )
        .unwrap();
        match submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected") {
            Err(error) => {
                assert!(
                    error.to_string().contains("TASK_SEAM_DISCONNECTED"),
                    "{case}: {error}"
                );
                assert_eq!(
                    tasks::store_for_task(root.root(), "forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .inspect("forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .stage,
                    "contract"
                );
            }
            Ok(_) => failures.push(format!(
                "{case}: shared primary index attested a disconnected claimed worktree despite fresh local index"
            )),
        }
    }
    let removed = TaskHarness::remove_worktree(root.root(), &worktree);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
