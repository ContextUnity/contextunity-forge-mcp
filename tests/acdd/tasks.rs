use super::support::*;

#[test]
fn task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks() {
    use contextunity_forge_mcp::{core::response::ResponsePolicy, mcp::response};

    for action in [
        "sync",
        "sync_many",
        "claim",
        "minimal",
        "next",
        "context",
        "reader",
    ] {
        let (root, mut store, _) = fixture();
        let task_id = "forge-mcp/m-test:first";
        if action == "reader" {
            let tx = store
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            let reader = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).expect(
                "initialized readers must open while another connection holds the writer lock",
            );
            assert_eq!(reader.inspect(task_id).unwrap().task_id, task_id);
            tx.rollback().unwrap();
            continue;
        }
        let large = if action == "sync_many" {
            "isolated".into()
        } else {
            "\\\"λ\n".repeat(20_000)
        };
        let mut spec = SPEC.replace(
            "invariants: [isolated]",
            &format!("invariants: [{}]", serde_json::to_string(&large).unwrap()),
        );
        if action == "sync_many" {
            for index in 0..200 {
                spec.push_str(&format!("```yaml\ntask_ref: extra-{index}\ntarget: Extra task\nproof_policy: seam-test-first\nscope: [src/extra-{index}.rs]\n```\n"));
            }
        }
        root.write("docs/010-test.md", &spec);
        let milestone = Milestone::parse(&spec, "forge-mcp").unwrap();
        // Use the same initial contract with large, valid authority metadata.
        store
            .delete(
                None,
                Some("docs/010-test.md"),
                true,
                contextunity_forge_mcp::db::tasks_store::now(),
            )
            .unwrap();
        store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
        let value = if matches!(action, "claim" | "minimal") {
            tasks::claim(
                &root.0,
                tasks::Claim {
                    task_id: task_id.into(),
                    stage: "contract".into(),
                    worker_id: "builder".into(),
                    worktree: root.0.to_string_lossy().into_owned(),
                    bundle: Some(action != "minimal"),
                },
            )
            .unwrap()
        } else {
            tasks::manage(
                &root.0,
                tasks::Manage {
                    action: match action {
                        "sync" | "sync_many" => tasks::ManageAction::Sync,
                        "next" => tasks::ManageAction::Next,
                        _ => tasks::ManageAction::Context,
                    },
                    milestone_ref: action
                        .starts_with("sync")
                        .then(|| "docs/010-test.md".into()),
                    task_id: (action == "context").then(|| task_id.into()),
                    worker_id: (action == "next").then(|| "builder".into()),
                    worktree: (action == "next").then(|| root.0.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let wire = response::result(Ok(value.clone()), &ResponsePolicy::default());
        assert_ne!(
            wire.is_error,
            Some(true),
            "{action}: mutation must return usable success: {wire:?}"
        );
        assert!(response::serialized_bytes(&wire) < 65536, "{action}");
        assert!(value.to_string().len() <= 24 * 1024, "{action}");
        if action == "sync_many" {
            assert_eq!(value["total"], 202);
            assert_eq!(value["byte_limited"], true);
            assert!(value["returned"].as_u64().unwrap() < 202);
            let count: usize = store
                .connection
                .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 202, "response bounding must not skip sync effects");
        } else if !action.starts_with("sync") {
            assert_eq!(value["byte_limited"], true);
            assert!(!value["omitted_fields"].as_array().unwrap().is_empty());
        }
        assert_eq!(
            store.inspect(task_id).unwrap().applicable_invariants,
            vec![large]
        );
        if matches!(action, "claim" | "minimal" | "next") {
            assert_eq!(value["task_id"], task_id);
            assert_eq!(
                value["claim_revision"],
                store.inspect(task_id).unwrap().claim_revision
            );
            assert!(value["workflow_guidance"].is_object());
        }
    }
}

fn prepare_indexed_boundary_workspace(
    api: &str,
    caller: &str,
    seam: &str,
    expected_target: &str,
    test_helper_source: Option<&str>,
    expected_intermediate: Option<&str>,
) -> ScopedWorkspace {
    let root = ScopedWorkspace::new("forge_current_boundary");
    root.write(
        "Cargo.toml",
        "[package]\nname = \"boundary_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write(
        "src/lib.rs",
        if expected_intermediate.is_some() {
            "pub mod api;\npub mod caller;\npub mod other;\npub mod test_helpers;\n#[cfg(test)]\nmod seam;\n"
        } else {
            "pub mod api;\npub mod caller;\npub mod other;\n#[cfg(test)]\nmod seam;\n"
        },
    );
    root.write("src/api.rs", api);
    root.write("src/caller.rs", caller);
    root.write("src/other.rs", "pub fn route() -> usize { 2 }\n");
    root.write("src/seam.rs", seam);
    if let Some(test_helper_source) = test_helper_source {
        root.write("src/test_helpers.rs", test_helper_source);
    }
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/milestones/010-boundary.md", "---\nid: m-current-boundary\ntitle: Current boundary evidence\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve current seam and public signature boundaries\nproof_policy: seam-test-first\nscope: [src/api.rs, src/seam.rs]\nscope_roots: [src/, tests/]\n```\n");
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    let has_expected_witness = if let Some(intermediate) = expected_intermediate {
        let exact_witness: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM edges test_edge JOIN nodes test ON test.node_hash=test_edge.src_hash JOIN path_dictionary test_path ON test_path.path_id=test.path_id JOIN nodes helper ON helper.node_hash=test_edge.dst_hash JOIN path_dictionary helper_path ON helper_path.path_id=helper.path_id JOIN edges helper_edge ON helper_edge.src_hash=helper.node_hash JOIN nodes target ON target.node_hash=helper_edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE test_edge.kind='calls' AND helper_edge.kind='calls' AND test.name='test_connected' AND test.kind='function' AND test_path.path='src/seam.rs' AND test.is_test=1 AND helper.name=?2 AND helper.kind='function' AND helper_path.path='src/test_helpers.rs' AND helper.is_test=1 AND target.name=?1 AND target.kind='function' AND target_path.path='src/api.rs' AND target.is_test=0",
                [expected_target, intermediate],
                |row| row.get(0),
            )
            .unwrap();
        exact_witness > 0
    } else {
        let seam_depth: Option<i64> = connection
            .query_row(
                "WITH RECURSIVE reachable(node_hash,depth) AS (SELECT node_hash,0 FROM nodes WHERE name='test_connected' AND is_test=1 UNION SELECT edge.dst_hash,reachable.depth+1 FROM reachable JOIN edges edge ON edge.src_hash=reachable.node_hash WHERE reachable.depth<3 AND edge.kind IN ('calls','possible_calls')) SELECT MIN(reachable.depth) FROM reachable JOIN nodes target ON target.node_hash=reachable.node_hash JOIN path_dictionary path ON path.path_id=target.path_id WHERE path.path='src/api.rs' AND target.is_test=0 AND target.kind='function' AND target.name=?1",
                [expected_target],
                |row| row.get(0),
            )
            .unwrap();
        seam_depth.is_some_and(|depth| (1..=3).contains(&depth))
    };
    assert!(
        has_expected_witness,
        "fixture must persist the exact expected test-to-production call path"
    );
    drop(connection);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-boundary.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    root
}

fn indexed_boundary_workspace(
    api: &str,
    caller: &str,
    seam: &str,
    expected_target: &str,
) -> ScopedWorkspace {
    let root = prepare_indexed_boundary_workspace(api, caller, seam, expected_target, None, None);
    tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
    root
}

fn indexed_test_path_helper_workspace(api: &str, test_helper_source: &str) -> ScopedWorkspace {
    let root = prepare_indexed_boundary_workspace(
        api,
        "",
        "use crate::test_helpers::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n",
        "route",
        Some(test_helper_source),
        Some("helper"),
    );
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    root
}

fn submit_indexed_boundary(
    root: &ScopedWorkspace,
    stage: &str,
    seam: &str,
) -> anyhow::Result<Value> {
    let task_id = "forge-mcp/m-current-boundary:routing";
    let task = tasks::store_for_task(&root.0, task_id)?.inspect(task_id)?;
    let mut proof = evidence(&task);
    proof.proof = if stage == "contract" {
        json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}})
    } else {
        json!({"command_proof":{"command":"cargo test --test connected","exit_code":0,"tests_passed":1,"tests_failed":0}})
    };
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: stage.into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof)?,
            findings: None,
        },
    )
}

#[test]
fn build_gate_handles_file_retirement_and_preserves_caller_safety() {
    for staged in [false, true] {
        for refresh in [false, true] {
            for case in [
                "no-callers",
                "external-live",
                "admitted-caller",
                "removed-caller",
                "unadmitted-removed-caller",
            ] {
                let root = prepare_indexed_boundary_workspace(
                    "pub fn route() -> usize { 1 }\n",
                    if case == "no-callers" { "" } else { "use crate::api::route;\npub fn invoke() -> usize { route() }\n" },
                    "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
                    "route", None, None,
                );
                let task_id = "forge-mcp/m-current-boundary:routing";
                if matches!(case, "admitted-caller" | "removed-caller") {
                    tasks::manage(&root.0, serde_json::from_value(json!({"action":"extend_scope", "task_id":task_id, "paths":["src/caller.rs"]})).unwrap()).unwrap();
                }
                tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
                submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected").unwrap();
                tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
                std::fs::remove_file(root.0.join("src/api.rs")).unwrap();
                if case == "admitted-caller" {
                    root.write("src/caller.rs", "pub fn invoke() -> usize { 0 }\n");
                }
                if matches!(case, "removed-caller" | "unadmitted-removed-caller") {
                    std::fs::remove_file(root.0.join("src/caller.rs")).unwrap();
                }
                if staged {
                    let output = std::process::Command::new("git")
                        .args(["add", "-u", "src"])
                        .current_dir(&root.0)
                        .output()
                        .unwrap();
                    assert!(output.status.success());
                }
                if refresh {
                    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
                }
                let result = submit_indexed_boundary(&root, "build", "src/seam.rs::test_connected");
                if matches!(case, "external-live" | "unadmitted-removed-caller") {
                    let error = result.unwrap_err().to_string();
                    assert!(
                        error.contains("TASK_SCOPE_INCOMPLETE")
                            && error.contains("src/caller.rs")
                            && error.contains("route"),
                        "{case}/{staged}/{refresh}: {error}"
                    );
                    let task = tasks::store_for_task(&root.0, task_id)
                        .unwrap()
                        .inspect(task_id)
                        .unwrap();
                    assert_eq!(task.stage, "build");
                    assert_eq!(task.status, "in_progress");
                } else {
                    assert!(result.is_ok(), "{case}/{staged}/{refresh}: {result:?}");
                    assert_eq!(result.unwrap()["stage"], "review");
                }
            }
        }
    }
}

#[test]
fn contract_rejects_retirement_without_baseline_caller_evidence() {
    for staged in [false, true] {
        for refresh in [false, true] {
            for external_caller in [false, true] {
                let root = prepare_indexed_boundary_workspace(
                "pub fn route() -> usize { 1 }\n",
                if external_caller { "use crate::api::route;\npub fn invoke() -> usize { route() }\n" } else { "" },
                "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
                "route", None, None,
            );
                let task_id = "forge-mcp/m-current-boundary:routing";
                tasks::manage(&root.0, serde_json::from_value(json!({"action":"extend_scope", "task_id":task_id, "paths":["src/other.rs"]})).unwrap()).unwrap();
                root.write("src/seam.rs", "use crate::other::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 2); }\n");
                commit_fixture_baseline(&root);
                writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
                std::fs::remove_file(root.0.join("src/api.rs")).unwrap();
                if staged {
                    assert!(std::process::Command::new("git")
                        .args(["add", "-u", "src"])
                        .current_dir(&root.0)
                        .status()
                        .unwrap()
                        .success());
                }
                if refresh {
                    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
                }
                tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
                let result =
                    submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected");
                if !refresh && !external_caller {
                    assert!(
                        result.is_ok(),
                        "{staged}/{refresh}/{external_caller}: {result:?}"
                    );
                } else {
                    let error = result.unwrap_err().to_string();
                    assert!(
                        error.contains("TASK_SCOPE_INCOMPLETE") && error.contains("route"),
                        "{staged}/{refresh}/{external_caller}: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn build_revalidates_legacy_empty_baseline_caller_evidence() {
    for indexed_target in [false, true] {
        for external_caller in [false, true] {
            let root = prepare_indexed_boundary_workspace(
                "pub fn route() -> usize { 1 }\n",
                if external_caller { "use crate::api::route;\npub fn invoke() -> usize { route() }\n" } else { "" },
                "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
                "route", None, None,
            );
            let task_id = "forge-mcp/m-current-boundary:routing";
            tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
            submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected").unwrap();
            {
                let store = tasks::store_for_task(&root.0, task_id).unwrap();
                let (revision, result): (u64, String) = store.connection.query_row(
                    "SELECT revision,result FROM task_submissions WHERE task_id=?1 ORDER BY revision DESC LIMIT 1",
                    [task_id], |row| Ok((row.get(0)?, row.get(1)?)),
                ).unwrap();
                let mut result: Value = serde_json::from_str(&result).unwrap();
                for entry in result["boundary_callers"]["entries"]
                    .as_array_mut()
                    .unwrap()
                {
                    entry.as_object_mut().unwrap().remove("target_present");
                }
                store
                    .connection
                    .execute(
                        "UPDATE task_submissions SET result=?1 WHERE task_id=?2 AND revision=?3",
                        rusqlite::params![result.to_string(), task_id, revision],
                    )
                    .unwrap();
            }
            tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
            std::fs::remove_file(root.0.join("src/api.rs")).unwrap();
            if !indexed_target {
                writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
            }
            let result = submit_indexed_boundary(&root, "build", "src/seam.rs::test_connected");
            if indexed_target && !external_caller {
                assert!(result.is_ok(), "{result:?}");
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("TASK_SCOPE_INCOMPLETE"), "{error}");
                if external_caller {
                    assert!(error.contains("src/caller.rs"), "{error}");
                }
            }
        }
    }
}

#[test]
fn signature_boundary_protects_implicitly_public_trait_methods() {
    let anchor = "pub fn anchor() -> usize { 1 }\n";
    let connected =
        "use crate::api::anchor;\n#[test]\nfn test_connected() { let actual = anchor(); assert_eq!(actual, 1); }\n";
    let caller = "use crate::api::Router;\npub fn invoke_route(router: &dyn Router) -> usize { Router::route(router) }\n";
    let mut failures = Vec::new();
    for (case, baseline, changed, reject) in [
        ("default_trait_method", "pub trait Router { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> u64 { 1 } }", true),
        ("declared_trait_method", "pub trait Router { fn route(&self) -> usize; }", "pub trait Router { fn route(&self) -> u64; }", true),
        ("trait_body_only", "pub trait Router { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize { 2 } }", false),
        ("private_inherent_method", "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self, value: usize) -> usize { value } }", false),
        ("trait_impl_body_only", "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 2 } }", false),
        ("crate_visible_trait", "pub(crate) trait Router { fn route(&self) -> usize { 1 } }", "pub(crate) trait Router { fn route(&self) -> u64 { 1 } }", true),
        ("qualified_visible_trait", "pub(in crate) trait Router { fn route(&self) -> usize { 1 } }", "pub(in crate) trait Router { fn route(&self) -> u64 { 1 } }", true),
    ] {
        for stage in ["contract", "build"] {
            let root = indexed_boundary_workspace(
                &format!("{anchor}{baseline}\n"),
                caller,
                connected,
                "anchor",
            );
            let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
            let exact_calls: i64 = connection.query_row(
                "SELECT COUNT(*) FROM edges edge JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary tp ON tp.path_id=target.path_id JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN path_dictionary cp ON cp.path_id=caller.path_id WHERE edge.kind='calls' AND target.name='route' AND target.kind='method' AND tp.path='src/api.rs' AND caller.name='invoke_route' AND cp.path='src/caller.rs' AND caller.is_test=0",
                [], |row| row.get(0)).unwrap();
            assert!(exact_calls > 0, "{case}/{stage}: fixture must persist the exact external UFCS call");
            drop(connection);
            if stage == "build" {
                submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected").unwrap();
                tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
            }
            root.write("src/api.rs", &format!("{anchor}{changed}\n"));
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
            let result = submit_indexed_boundary(&root, stage, "src/seam.rs::test_connected");
            match (reject, result) {
                (true, Err(error)) => {
                    assert!(error.to_string().contains("TASK_SCOPE_INCOMPLETE"), "{case}/{stage}: {error}");
                    assert!(error.to_string().contains("src/caller.rs"), "{case}/{stage}: {error}");
                    assert_eq!(tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing").unwrap().inspect("forge-mcp/m-current-boundary:routing").unwrap().stage, stage);
                }
                (true, Ok(_)) => failures.push(format!("{case}/{stage}: changed implicitly public signature accepted despite exact external UFCS caller")),
                (false, Err(error)) => failures.push(format!("{case}/{stage}: unchanged public signature rejected: {error}")),
                (false, Ok(_)) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn contract_seam_accepts_root_cargo_package_imports_from_integration_tests() {
    let root = prepare_indexed_boundary_workspace(
        "pub fn route() -> usize { 1 }\n",
        "",
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
        "route",
        None,
        None,
    );
    root.write(
        "tests/connected.rs",
        "use boundary_fixture::api::route;\n#[test]\nfn integration_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    ).unwrap();
    submit_indexed_boundary(&root, "contract", "tests/connected.rs").unwrap();
    assert_eq!(
        tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
            .unwrap()
            .inspect("forge-mcp/m-current-boundary:routing")
            .unwrap()
            .stage,
        "build"
    );
}

#[test]
fn contract_seam_requires_current_indexed_paths_at_every_hop() {
    let api = "pub fn route() -> usize { 1 }\n";
    let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\npub fn outer() -> usize { helper() }\n";
    let direct =
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n";
    let indirect =
        "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n";
    let mut failures = Vec::new();
    for (case, seam, refresh, reject) in [
        ("fresh_direct", "src/seam.rs::test_connected", false, false),
        (
            "fresh_two_hops",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
        ("three_hops", "src/seam.rs::test_connected", false, true),
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
        } else if case == "three_hops" {
            direct
                .replace("api::route", "caller::outer")
                .replace("route()", "outer()")
        } else {
            direct.to_owned()
        };
        let root = indexed_boundary_workspace(api, caller, &initial, "route");
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
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        }
        let result = submit_indexed_boundary(&root, "contract", seam);
        match (reject, result) {
            (true, Err(error)) => {
                if !error.to_string().contains("TASK_SEAM_DISCONNECTED") {
                    failures.push(format!(
                        "{case}: stale seam reached a later boundary: {error}"
                    ));
                }
                assert_eq!(
                    tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .inspect("forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .stage,
                    "contract"
                );
            }
            (true, Ok(_)) => failures.push(format!(
                "{case}: accepted a seam without a current indexed one- or two-hop production path"
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

    let fresh = indexed_test_path_helper_workspace(api, test_helper_source);
    match submit_indexed_boundary(&fresh, "contract", "src/seam.rs::test_connected") {
        Ok(_) => {}
        Err(error) => failures.push(format!("fresh test-marked helper path rejected: {error}")),
    }

    let stale = indexed_test_path_helper_workspace(api, test_helper_source);
    stale.write(
        "src/test_helpers.rs",
        &test_helper_source.replace(
            "pub fn helper() -> usize { route() }",
            "fn helper() -> usize { 1 }",
        ),
    );
    match submit_indexed_boundary(&stale, "contract", "src/seam.rs::test_connected") {
        Err(error) if error.to_string().contains("TASK_SEAM_DISCONNECTED") => {}
        Err(error) => failures.push(format!(
            "stale helper failed at the wrong boundary: {error}"
        )),
        Ok(_) => {
            failures.push("stale test-marked helper path was accepted without reindexing".into())
        }
    }
    let stale_stage = tasks::store_for_task(&stale.0, "forge-mcp/m-current-boundary:routing")
        .unwrap()
        .inspect("forge-mcp/m-current-boundary:routing")
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
    let mut failures = Vec::new();
    for (case, disconnect_helper) in [("stale_test", false), ("stale_helper", true)] {
        let api = "pub fn route() -> usize { 1 }\n";
        let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\n";
        let seam = if disconnect_helper {
            "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n"
        } else {
            "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n"
        };
        let root = prepare_indexed_boundary_workspace(api, caller, seam, "route", None, None);
        let worktree = root.0.join("claimed-worktree");
        let added = std::process::Command::new("git")
            .args(["worktree", "add", "--quiet", "-b", "claimed-boundary"])
            .arg(&worktree)
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":worktree})).unwrap()).unwrap();
        if disconnect_helper {
            std::fs::write(
                worktree.join("src/caller.rs"),
                "pub fn helper() -> usize { 2 }\n",
            )
            .unwrap();
        } else {
            std::fs::write(
                worktree.join("src/seam.rs"),
                "#[test]\nfn test_connected() { assert_eq!(1, 2); }\n",
            )
            .unwrap();
        }
        writer::build(&worktree, &worktree.join(".forge/code-map.sqlite"), None).unwrap();
        let result = submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected");
        let Err(error) = result else {
            failures.push(format!("{case}: shared primary index attested a disconnected claimed worktree despite fresh local index"));
            continue;
        };
        assert!(
            error.to_string().contains("TASK_SEAM_DISCONNECTED"),
            "{case}: {error}"
        );
        assert_eq!(
            tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
                .unwrap()
                .inspect("forge-mcp/m-current-boundary:routing")
                .unwrap()
                .stage,
            "contract"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn commit_fixture_baseline(root: &ScopedWorkspace) {
    let added = std::process::Command::new("git")
        .args(["add", "--all"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(added.status.success());
    let committed = std::process::Command::new("git")
        .args(["commit", "--quiet", "-m", "fixture source baseline"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "could not commit fixture baseline: {}",
        String::from_utf8_lossy(&committed.stderr)
    );
}

fn add_boundary_edge_fixtures(root: &ScopedWorkspace) {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES('acdd boundary fixture')",
            [],
        )
        .unwrap();
    let evidence: i64 = connection
        .query_row(
            "SELECT evidence_id FROM coverage_evidence WHERE evidence='acdd boundary fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for (path, name, relation) in [
        ("src/possible.rs", "possible_route", "possible_calls"),
        ("src/implements.rs", "implements_route", "implements"),
        ("src/overrides.rs", "overrides_route", "overrides"),
    ] {
        let inserted = connection
            .execute(
                "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) \
                 SELECT caller.node_hash,?1,target.node_hash,caller.path_id,caller.line,?2,?2,1 \
                 FROM nodes caller JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
                 JOIN nodes target ON target.name='route_task' AND target.line=1 \
                 JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
                 WHERE caller_path.path=?3 AND caller.name=?4 AND target_path.path='src/routing.rs'",
                rusqlite::params![relation, evidence, path, name],
            )
            .unwrap();
        assert_eq!(
            inserted, 1,
            "fixture relation {relation} must target the public route_task symbol"
        );
    }
}

#[test]
fn tiered_task_context_enforces_indexed_seams_and_signature_boundaries() {
    let mut failures = Vec::new();
    for (stage, seam, change_signature, admit_caller, expected_error) in [
        (
            "contract",
            "tests/disconnected.rs",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_unrelated",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_missing",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "tests/deleted.rs::test_deleted",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_connected",
            true,
            false,
            Some("TASK_SCOPE_INCOMPLETE"),
        ),
        (
            "build",
            "src/routing.rs::test_connected",
            true,
            false,
            Some("TASK_SCOPE_INCOMPLETE"),
        ),
        ("build", "src/routing.rs::test_connected", true, true, None),
    ] {
        let root = ScopedWorkspace::new("forge_tiered_boundary");
        root.write("src/lib.rs", "pub mod routing;\npub mod caller;\npub mod possible;\npub mod implements;\npub mod overrides;\n");
        root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\npub mod archive { pub fn route_task() -> usize { 2 } }\npub fn test_only_route() -> usize { 3 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() {\n        let actual = route_task();\n        assert_eq!(actual, 1);\n    }\n    #[test]\n    fn test_unrelated() {\n        let actual = 3;\n        assert_eq!(actual, 3);\n    }\n}\n");
        root.write(
            "Cargo.toml",
            "[package]\nname = \"tiered_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        root.write(
            "src/caller.rs",
            "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task() }\n#[cfg(test)]\nmod tests {\n    use crate::caller::invoke_route;\n    #[test]\n    fn test_indirect() {\n        let actual = invoke_route();\n        assert_eq!(actual, 1);\n    }\n}\n",
        );
        root.write(
            "src/possible.rs",
            "pub fn possible_route() -> usize { 1 }\n",
        );
        root.write(
            "src/implements.rs",
            "pub fn implements_route() -> usize { 1 }\n",
        );
        root.write(
            "src/overrides.rs",
            "pub fn overrides_route() -> usize { 1 }\n",
        );
        root.write("tests/connected.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
        root.write("tests/acdd.rs", "mod tasks;\n");
        root.write("tests/acdd/tasks.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_nested_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
        root.write(
            "tests/disconnected.rs",
            "#[test]\nfn test_disconnected() { assert_eq!(1, 1); }\n",
        );
        root.write("tests/external.rs", "use tiered_fixture::routing::test_only_route;\n#[test]\nfn external_test_only_caller() { let actual = test_only_route(); assert_eq!(actual, 3); }\n");
        root.write(
            "AGENTS.md",
            "# Instructions\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write(
            "forge-mcp.yaml",
            "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write("docs/adr/001-routing.md", "---\ntitle: Routing invariants\ndoc_type: adr\nstatus: accepted\n---\n# Routing invariants\n- INV-ROUTING: [`src.routing.route_task`](../../src/routing.rs#L1) remains observable through the public seam.\n- Decision: Admit every caller before changing the public signature.\n");
        let caller_scope = if admit_caller {
            ", src/caller.rs, src/possible.rs, src/implements.rs, src/overrides.rs"
        } else {
            ""
        };
        let specification = format!("---\nid: m-tiered\ntitle: Tiered boundary gates\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve the routing boundary\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs, tests/acdd/tasks.rs{caller_scope}]\nscope_roots: [src/, tests/]\n```\n");
        root.write("docs/milestones/010-tiered.md", &specification);
        commit_fixture_baseline(&root);
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        add_boundary_edge_fixtures(&root);
        tasks::manage(
            &root.0,
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let task_id = "forge-mcp/m-tiered:routing";
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
        let canonical_command =
            "cargo clippy --all-targets --all-features -- -D warnings && cargo test";
        if !profile
            .commands
            .values()
            .any(|command| command.command() == canonical_command)
        {
            failures.push(format!("{stage}/{seam}: default Rust profile must register static checks before cargo test"));
        }
        if !profile
            .commands
            .get("test")
            .is_some_and(|command| command.command() == "cargo test")
        {
            failures.push(format!("{stage}/{seam}: default Rust profile must retain the registered cargo test compatibility command"));
        }
        if profile.roles.len() != 3 {
            failures.push(format!(
                "{stage}/{seam}: default profile must have exactly three execution roles"
            ));
        }
        for (gate, role, model) in [
            ("contract", "contract_author", "sol-6.1:high"),
            ("build", "builder", "gpt6-luna:xhigh"),
            ("review", "independent_reviewer", "sol-6.1:xhigh"),
            ("deliver", "independent_reviewer", "sol-6.1:xhigh"),
        ] {
            let role_definition = profile.role_for_stage(gate);
            if !role_definition.is_some_and(|(name, definition)| {
                name == role
                    && definition.effective_reuse_on_reject()
                    && definition.effective_mode()
                        == contextunity_forge_mcp::core::tasks::profile::RoleExecutionMode::Subagent
                    && definition
                        .models
                        .iter()
                        .any(|entry| entry.formatted() == model)
            }) {
                failures.push(format!("{stage}/{seam}: {gate} must use {role} with {model} and reusable worker authority"));
            }
        }
        let mut claim = tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
        let guidance = &claim["workflow_guidance"];
        if !guidance["rules"].as_array().unwrap().iter().any(|rule| {
            rule.as_str()
                .is_some_and(|rule| rule.starts_with("NO_PREFLIGHT_SUBAGENTS:"))
        }) {
            failures.push(format!(
                "{stage}/{seam}: task guidance must prohibit preflight subagents"
            ));
        }
        if !guidance["steps"].as_array().unwrap().iter().any(|step| {
            step.as_str()
                .is_some_and(|step| step.contains("gate=\"build\"") || step.contains("gate=build"))
        }) {
            failures.push(format!(
                "{stage}/{seam}: contract steps must require the build-targeted blackboard handoff"
            ));
        }
        let template = &guidance["expected_evidence_template"];
        if !template.is_object() || !template.to_string().contains(task_id) {
            failures.push(format!(
                "{stage}/{seam}: claim must contain a task-bound expected evidence template"
            ));
        }
        let bundle = &claim["context_bundle"];
        if !bundle["adrs"].as_array().unwrap().iter().any(|adr| {
            adr["invariants_summary"].as_array().is_some_and(|summary| {
                summary.len() <= 5
                    && summary.iter().any(|line| {
                        line.as_str()
                            .is_some_and(|line| line.contains("INV-ROUTING"))
                    })
                    && summary
                        .iter()
                        .any(|line| line.as_str().is_some_and(|line| line.contains("Decision:")))
            })
        }) {
            failures.push(format!(
                "{stage}/{seam}: governing ADR invariants must be inlined"
            ));
        }
        let callers = bundle["external_callers"].as_array().unwrap();
        if callers
            .iter()
            .any(|caller| caller["path"] == "tests/external.rs")
        {
            failures.push(format!(
                "{stage}/{seam}: external boundary context must omit test callers"
            ));
        }
        if !admit_caller
            && ![
                "src/caller.rs",
                "src/possible.rs",
                "src/implements.rs",
                "src/overrides.rs",
            ]
            .iter()
            .all(|path| callers.iter().any(|caller| caller["path"] == *path))
        {
            failures.push(format!(
                "{stage}/{seam}: boundary context must include callers across all indexed edge kinds"
            ));
        }
        if !bundle["covering_tests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|test| test["path"] == "src/caller.rs")
        {
            failures.push(format!(
                "{stage}/{seam}: covering tests must follow two-hop production calls"
            ));
        }
        tasks::blackboard(&root.0, serde_json::from_value(json!({"action":"post","task_id":task_id,"topic":"draft","gate":"build","payload":"Keep route_task callers in scope when changing its signature; use tests/connected.rs."})).unwrap(), "contract-author").unwrap();
        if stage == "build" {
            let task = tasks::store_for_task(&root.0, task_id)
                .unwrap()
                .inspect(task_id)
                .unwrap();
            let mut proof = evidence(&task);
            proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
            tasks::submit(
                &root.0,
                tasks::Submit {
                    task_id: task_id.into(),
                    stage: "contract".into(),
                    action: tasks::Action::Pass,
                    evidence: serde_json::to_value(proof).unwrap(),
                    findings: None,
                },
            )
            .unwrap();
            claim = tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
            if !claim["workflow_guidance"]["blackboard_messages"]
                .as_array()
                .is_some_and(|messages| {
                    messages
                        .iter()
                        .any(|message| message["gate"] == "build" && message["topic"] == "draft")
                })
            {
                failures.push(format!(
                    "{stage}/{seam}: builder must receive the contract author's targeted handoff"
                ));
            }
            let scoped = &claim["workflow_guidance"]["scoped_commands"];
            if scoped.is_null()
                || !scoped.to_string().contains("connected")
                || !scoped.to_string().contains("--test acdd")
                || !scoped.to_string().contains(canonical_command)
                || !scoped.as_array().is_some_and(|entries| {
                    !entries.is_empty()
                        && entries.iter().all(|entry| {
                            entry["name"] == "rust-check-and-test"
                                && entry["command"].as_str().is_some_and(|command| {
                                    command.starts_with(canonical_command)
                                        && (command.ends_with("--test connected")
                                            || command.ends_with("--test acdd")
                                            || command.ends_with("--test external")
                                            || command.ends_with("--lib"))
                                })
                        })
                        && entries.iter().any(|entry| {
                            entry["test_path"] == "tests/acdd/tasks.rs"
                                && entry["command"]
                                    .as_str()
                                    .is_some_and(|command| command.ends_with("--test acdd"))
                        })
                        && entries.iter().any(|entry| {
                            entry["test_path"] == "tests/external.rs"
                                && entry["command"]
                                    .as_str()
                                    .is_some_and(|command| command.ends_with("--test external"))
                        })
                })
            {
                failures.push(format!("{stage}/{seam}: scoped commands must append seam and covering-test suffixes only to the canonical combined command: {scoped}"));
            }
        }
        if change_signature {
            root.write(
                "src/routing.rs",
                "pub fn route_task(value: usize) -> usize { value + 1 }\npub fn test_only_route(value: usize) -> usize { value + 3 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() {\n        let actual = route_task(0);\n        assert_eq!(actual, 1);\n    }\n}\n",
            );
            root.write("tests/connected.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
            root.write("tests/external.rs", "use tiered_fixture::routing::test_only_route;\n#[test]\nfn external_test_only_caller() { let actual = test_only_route(0); assert_eq!(actual, 3); }\n");
            if admit_caller {
                root.write("src/caller.rs", "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task(0) }\n#[cfg(test)]\nmod tests {\n    use crate::caller::invoke_route;\n    #[test]\n    fn test_indirect() {\n        let actual = invoke_route();\n        assert_eq!(actual, 1);\n    }\n}\n");
            }
        }
        let store = tasks::store_for_task(&root.0, task_id).unwrap();
        let task = store.inspect(task_id).unwrap();
        let mut proof = evidence(&task);
        if stage == "contract" {
            proof.proof = json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}});
        } else {
            let command = profile
                .commands
                .values()
                .find(|command| command.command() == canonical_command)
                .or_else(|| profile.commands.values().next())
                .unwrap()
                .command();
            proof.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
        }
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: stage.into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(proof).unwrap(),
                findings: None,
            },
        );
        match (expected_error, result) {
            (Some(code), Err(error)) => {
                assert!(error.to_string().contains(code), "{stage}/{seam}: {error}");
                if change_signature && !admit_caller {
                    for path in [
                        "src/caller.rs",
                        "src/possible.rs",
                        "src/implements.rs",
                        "src/overrides.rs",
                    ] {
                        assert!(
                            error.to_string().contains(path),
                            "{stage}/{seam}: signature gate omitted {path}: {error}"
                        );
                    }
                    assert!(
                        !error.to_string().contains("tests/external.rs"),
                        "test callers must not cause boundary rejection: {error}"
                    );
                }
                assert_eq!(
                    store.inspect(task_id).unwrap().stage,
                    stage,
                    "a rejected boundary must preserve the active gate"
                );
            }
            (Some(code), Ok(_)) => {
                failures.push(format!("{stage}/{seam}: must reject with {code}"))
            }
            (None, Err(error)) => failures.push(format!(
                "{stage}/{seam}: admitted caller signature change must pass: {error}"
            )),
            (None, Ok(_)) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn method_caller_edge_count(root: &ScopedWorkspace) -> i64 {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .query_row(
            "SELECT COUNT(*) FROM edges edge \
             JOIN nodes target ON target.node_hash=edge.dst_hash AND target.kind='method' AND target.name='route_task' \
             JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
             JOIN nodes caller ON caller.node_hash=edge.src_hash AND caller.name='invoke_route' AND caller.is_test=0 \
             JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
             WHERE target_path.path='src/routing.rs' AND caller_path.path='src/caller.rs' \
               AND edge.kind IN ('calls','imports','possible_calls','implements','overrides')",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn function_caller_edge_count(root: &ScopedWorkspace, path: &str, name: &str) -> i64 {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .query_row(
            "SELECT COUNT(*) FROM edges edge \
             JOIN nodes target ON target.node_hash=edge.dst_hash AND target.kind='function' AND target.name=?2 \
             JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
             JOIN nodes caller ON caller.node_hash=edge.src_hash AND caller.is_test=0 \
             JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
              WHERE target_path.path=?1 AND caller_path.path IN ('src/client.rs','src/client.ts','src/client.js','src/client.py') \
               AND edge.kind IN ('calls','imports','possible_calls','implements','overrides')",
            rusqlite::params![path, name],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn signature_boundary_uses_exact_method_baseline_across_body_edits_and_reindex() {
    let routing_baseline = r#"pub fn route_task() -> usize { 1 }
pub struct Router;
impl Router {
    pub fn route_task(&self) -> usize { 10 }
}
pub struct Archive;
impl Archive {
    pub fn route_task(&self) -> usize { 20 }
}
pub fn test_only_route() -> usize { 3 }
#[cfg(test)]
mod tests {
    use crate::routing::route_task;

    #[test]
    fn test_connected() {
        let connected_target = route_task();
        let router = crate::routing::Router;
        let actual = router.route_task();
        assert_eq!(connected_target, 1);
        assert_eq!(actual, 10);
    }
}
"#;
    let caller_baseline = "use crate::routing::Router;\nimpl Router {\n    pub fn invoke_route(&self) -> usize { self.route_task() }\n}\n";
    let connected_test_baseline = "#[test]\nfn test_connected() { let router = tiered_fixture::routing::Router; let actual = router.route_task(); assert_eq!(actual, 10); }\n";

    for (case, expected_error, refresh_index) in [
        ("same_named_methods", Some("TASK_SCOPE_INCOMPLETE"), false),
        ("body_only", None, false),
        ("refreshed_index", Some("TASK_SCOPE_INCOMPLETE"), true),
    ] {
        let root = ScopedWorkspace::new("forge_tiered_signature_identity");
        root.write("src/lib.rs", "pub mod routing;\npub mod caller;\n");
        root.write("src/routing.rs", routing_baseline);
        root.write("src/caller.rs", caller_baseline);
        root.write(
            "Cargo.toml",
            "[package]\nname = \"tiered_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        root.write("tests/connected.rs", connected_test_baseline);
        root.write("tests/acdd.rs", "mod tasks;\n");
        root.write(
            "tests/acdd/tasks.rs",
            "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
        );
        root.write(
            "AGENTS.md",
            "# Instructions\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write(
            "forge-mcp.yaml",
            "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write(
            "docs/milestones/010-tiered.md",
            "---\nid: m-tiered-signature\ntitle: Tiered signature boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: route-task\ntarget: Protect the exact public method signature\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
        );
        commit_fixture_baseline(&root);
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        let original_caller_edges = method_caller_edge_count(&root);
        assert!(
            original_caller_edges > 0,
            "fixture must index the external call to Router::route_task"
        );
        tasks::manage(
            &root.0,
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let task_id = "forge-mcp/m-tiered-signature:route-task";
        tasks::claim(
            &root.0,
            serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut contract_evidence = evidence(&task);
        contract_evidence.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "contract".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(contract_evidence).unwrap(),
                findings: None,
            },
        )
        .unwrap();
        let claim = tasks::claim(
            &root.0,
            serde_json::from_value(
                json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(claim["stage"], "build");

        let mut routing = routing_baseline.to_owned();
        let mut caller = caller_baseline.to_owned();
        let mut connected_test = connected_test_baseline.to_owned();
        match case {
            "same_named_methods" | "refreshed_index" => {
                routing = routing.replace(
                    "impl Router {\n    pub fn route_task(&self) -> usize { 10 }",
                    "impl Router {\n    pub fn route_task(&self, value: usize) -> usize { 10 + value }",
                );
                assert_ne!(routing, routing_baseline, "Router method must change");
                caller = caller.replace("self.route_task()", "self.route_task(0)");
                connected_test =
                    connected_test.replace("router.route_task()", "router.route_task(0)");
            }
            "body_only" => {
                routing = routing.replace(
                    "impl Router {\n    pub fn route_task(&self) -> usize { 10 }",
                    "impl Router {\n    pub fn route_task(&self) -> usize { 11 }",
                );
                assert_ne!(routing, routing_baseline, "Router method body must change");
                connected_test =
                    connected_test.replace("assert_eq!(actual, 10)", "assert_eq!(actual, 11)");
            }
            _ => unreachable!("all signature scenarios are listed above"),
        }
        assert!(
            routing.contains("impl Archive {\n    pub fn route_task(&self) -> usize { 20 }"),
            "{case}: unchanged same-named Archive method must remain in the fixture"
        );
        if case == "body_only" {
            assert!(
                routing.contains("impl Router {\n    pub fn route_task(&self) -> usize { 11 }"),
                "body-only case must preserve Router's qualified signature"
            );
        } else {
            assert!(
                routing.contains(
                    "impl Router {\n    pub fn route_task(&self, value: usize) -> usize { 10 + value }"
                ),
                "{case}: only the externally called Router method signature changes"
            );
        }
        root.write("src/routing.rs", &routing);
        root.write("src/caller.rs", &caller);
        root.write("tests/connected.rs", &connected_test);

        if refresh_index {
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
            let refreshed_caller_edges = method_caller_edge_count(&root);
            assert!(
                refreshed_caller_edges >= original_caller_edges,
                "reindex must preserve the external Router::route_task caller edge: before={original_caller_edges}, after={refreshed_caller_edges}"
            );
        }

        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut build_evidence = evidence(&task);
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
        let command = profile.commands.get("test").unwrap().command();
        build_evidence.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "build".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(build_evidence).unwrap(),
                findings: None,
            },
        );
        match (expected_error, result) {
            (Some(code), Err(error)) => {
                assert!(error.to_string().contains(code), "{case}: {error}");
                assert!(
                    error.to_string().contains("src/caller.rs"),
                    "{case}: external method caller must remain in the rejection: {error}"
                );
            }
            (Some(code), Ok(_)) => panic!("{case}: expected rejection with {code}"),
            (None, Err(error)) => panic!("{case}: body-only edit must pass: {error}"),
            (None, Ok(_)) => {}
        }
    }
}

#[test]
fn signature_boundary_keeps_contract_acceptance_callers_after_symbol_rename_and_reindex() {
    let root = ScopedWorkspace::new("forge_tiered_renamed_boundary");
    root.write("src/lib.rs", "pub mod api;\npub mod client;\n");
    root.write(
        "src/api.rs",
        "pub fn fetch(id: usize) -> usize { id }\n#[cfg(test)]\nmod tests { use crate::api::fetch; #[test] fn test_connected() { let actual = fetch(1); assert_eq!(actual, 1); } }\n",
    );
    root.write(
        "src/client.rs",
        "use crate::api::fetch;\npub fn load() -> usize { fetch(0) }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_rename_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_rename_fixture::api::fetch;\n#[test]\nfn test_connected() { let actual = fetch(1); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(
        "docs/milestones/010-tiered-rename.md",
        "---\nid: m-tiered-rename\ntitle: Renamed API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: rename-fetch\ntarget: Preserve every caller when renaming a public API\nproof_policy: seam-test-first\nscope: [src/api.rs, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let baseline_edges = function_caller_edge_count(&root, "src/api.rs", "fetch");
    assert!(
        baseline_edges > 0,
        "baseline index must include src/client.rs -> fetch"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-rename.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-rename:rename-fetch";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/api.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    let task_store = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let accepted_contract: String = task_store
        .query_row(
            "SELECT result FROM task_submissions WHERE task_id=?1 ORDER BY revision DESC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let accepted_contract: serde_json::Value = serde_json::from_str(&accepted_contract).unwrap();
    assert!(
        accepted_contract["boundary_callers"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["path"] == "src/api.rs"
                    && entry["name"] == "fetch"
                    && entry["caller_paths"]
                        .as_array()
                        .is_some_and(|paths| paths.iter().any(|path| path == "src/client.rs"))
            }),
        "contract acceptance must persist the stable-baseline external caller path"
    );
    let baseline = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(baseline.status.success());
    assert_eq!(
        accepted_contract["boundary_callers"]["baseline_commit"],
        String::from_utf8_lossy(&baseline.stdout).trim(),
        "caller evidence must be pinned to the Git baseline accepted by the contract gate"
    );
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.rs",
        "pub fn fetch_by_id(id: usize) -> usize { id }\n#[cfg(test)]\nmod tests { use crate::api::fetch_by_id; #[test] fn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); } }\n",
    );
    root.write("tests/connected.rs", "use tiered_rename_fixture::api::fetch_by_id;\n#[test]\nfn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); }\n");
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let old_edges_after_reindex = function_caller_edge_count(&root, "src/api.rs", "fetch");
    assert_eq!(
        old_edges_after_reindex, 0,
        "reindex must remove every edge to the renamed fetch symbol before boundary validation"
    );

    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error = result.expect_err(
        "contract-accepted external callers must survive an API rename and index rebuild",
    );
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.rs"),
        "saved baseline caller path must explain the rejection: {error}"
    );
}

#[test]
fn signature_boundary_covers_exported_typescript_callables() {
    let root = ScopedWorkspace::new("forge_tiered_typescript_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write(
        "src/api.ts",
        "export function submit(id: number): number { return id; }\n",
    );
    root.write(
        "src/client.ts",
        "import { submit } from './api';\nexport function load(): number { return submit(1); }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_typescript_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_typescript_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(
        "docs/milestones/010-tiered-typescript.md",
        "---\nid: m-tiered-typescript\ntitle: TypeScript API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-submit\ntarget: Protect exported TypeScript callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.ts, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.ts", "submit") > 0,
        "baseline index must resolve the TypeScript external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-typescript.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-typescript:protect-submit";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.ts",
        "export function submit(id: string): number { return id.length; }\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.ts", "submit") > 0,
        "reindex must retain the caller edge to the same exported symbol"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("exported TypeScript API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.ts"),
        "external TypeScript caller must explain the rejection: {error}"
    );
}

#[test]
fn signature_boundary_covers_public_python_callables() {
    let root = ScopedWorkspace::new("forge_tiered_python_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write(
        "src/api.py",
        "def fetch(id: int) -> int:\n    return id\n\ndef test_connected():\n    actual = fetch(1)\n    assert actual == 1\n",
    );
    root.write(
        "src/client.py",
        "from api import fetch\n\ndef load():\n    return fetch(0)\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_python_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_python_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(
        "docs/milestones/010-tiered-python.md",
        "---\nid: m-tiered-python\ntitle: Python API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-fetch\ntarget: Protect public Python callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.py, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.py", "fetch") > 0,
        "baseline index must resolve the Python external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-python.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-python:protect-fetch";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.py",
        "def fetch(id: str) -> int:\n    return len(id)\n\ndef test_connected():\n    actual = fetch(\"1\")\n    assert actual == 1\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.py", "fetch") > 0,
        "reindex must retain the caller edge to the same public Python callable"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("public Python API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.py"),
        "external Python caller must explain the rejection: {error}"
    );
}

#[test]
fn signature_boundary_covers_exported_javascript_callables() {
    let root = ScopedWorkspace::new("forge_tiered_javascript_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write("src/api.js", "export function submit(id) { return id; }\n");
    root.write(
        "src/client.js",
        "import { submit } from './api.js';\nexport function load() { return submit(1); }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_javascript_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_javascript_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(
        "docs/milestones/010-tiered-javascript.md",
        "---\nid: m-tiered-javascript\ntitle: JavaScript API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-submit\ntarget: Protect exported JavaScript callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.js, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.js", "submit") > 0,
        "baseline index must resolve the JavaScript external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-javascript.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-javascript:protect-submit";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.js",
        "export function submit(id, label) { return id; }\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.js", "submit") > 0,
        "reindex must retain the caller edge to the same exported JavaScript callable"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("exported JavaScript API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.js"),
        "external JavaScript caller must explain the rejection: {error}"
    );
}

#[test]
fn seam_scope_directory_prefix_treats_sql_wildcards_as_literal() {
    for (case, scope_dir, actual_dir) in [
        ("underscore", "api_v1", "apiXv1"),
        ("percent", "api%v1", "apiXYZv1"),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_tiered_literal_scope_{case}"));
        root.write("src/lib.rs", &format!("pub mod {actual_dir};\n"));
        root.write(
            &format!("src/{scope_dir}/allowed.rs"),
            "pub fn placeholder() {}\n",
        );
        root.write(&format!("src/{actual_dir}/mod.rs"), "pub mod hidden;\n");
        root.write(
            &format!("src/{actual_dir}/hidden.rs"),
            &format!(
                "pub fn route_task() -> usize {{ 1 }}\n#[cfg(test)]\nmod tests {{ use crate::{actual_dir}::hidden::route_task; #[test] fn test_connected() {{ let actual = route_task(); assert_eq!(actual, 1); }} }}\n"
            ),
        );
        root.write(
            "Cargo.toml",
            "[package]\nname = \"tiered_literal_scope_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        root.write("tests/acdd.rs", "mod tasks;\n");
        root.write(
            "tests/acdd/tasks.rs",
            "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
        );
        root.write(
            "AGENTS.md",
            "# Instructions\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write(
            "forge-mcp.yaml",
            "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let milestone_ref = format!("docs/milestones/010-tiered-literal-{case}.md");
        let milestone_id = format!("m-tiered-literal-{case}");
        let task_ref = format!("literal-scope-{case}");
        root.write(
            &milestone_ref,
            &format!(
                "---\nid: {milestone_id}\ntitle: Literal directory scope\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: {task_ref}\ntarget: Keep directory scope matching literal\nproof_policy: seam-test-first\nscope: [src/{scope_dir}/, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n"
            ),
        );
        commit_fixture_baseline(&root);
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref})).unwrap(),
        )
        .unwrap();
        let task_id = format!("forge-mcp/{milestone_id}:{task_ref}");
        tasks::claim(
            &root.0,
            serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, &task_id)
            .unwrap()
            .inspect(&task_id)
            .unwrap();
        let mut contract = evidence(&task);
        contract.proof = json!({"contract_proof":{"seam_test_ref":format!("src/{actual_dir}/hidden.rs::test_connected"),"red_exit_code":101}});
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.clone(),
                stage: "contract".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(contract).unwrap(),
                findings: None,
            },
        );
        let error = result.expect_err(&format!(
            "src/{actual_dir}/ must not match literal directory src/{scope_dir}/"
        ));
        assert!(
            error.to_string().contains("TASK_SEAM_DISCONNECTED"),
            "{case}: {error}"
        );
    }
}

#[test]
fn boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths() {
    let root = ScopedWorkspace::new("forge_tiered_alias_proof_kinds");
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_alias_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("src/lib.rs", "pub mod routing;\npub mod caller;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n}\n");
    root.write(
        "src/caller.rs",
        "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task() }\n",
    );
    root.write("tests/connected.rs", "use tiered_alias_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write("forge-mcp.yaml", "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n");
    root.write(
        "custom_profile.yaml",
        "gates:\n  - id: spec\n    role: contract_author\n    proof: contract\n    sha_snapshot: true\n  - id: implement\n    role: builder\n    proof: command\n    sha_snapshot: true\n  - id: inspect\n    role: independent_reviewer\n    proof: review\n    reject_to: implement\n    independent_from: implement\n    contours: standard\n  - id: archive\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [inspect]\n    reject_to: implement\ncommands:\n  rust-check-and-test:\n    command: pytest\n    description: Preserve ordinary scoped paths for non-Cargo profiles.\n",
    );
    let milestone = "---\nid: m-tiered-alias\ntitle: Proof-kind aliases\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve routing API boundaries\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs]\nscope_roots: [src/, tests/]\n```\n";
    root.write("docs/milestones/010-tiered-alias.md", milestone);
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-alias.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-alias:routing";
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"spec","worker_id":"spec-author","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();
    let store = tasks::store_for_task(&root.0, task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let mut proof = evidence(&task);
    proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "spec".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let claim = tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"implement","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();
    let scoped = &claim["workflow_guidance"]["scoped_commands"];
    assert!(
        scoped.as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["name"] == "rust-check-and-test"
                    && entry["test_path"] == "src/routing.rs"
                    && entry["command"] == "pytest src/routing.rs"
            })
        }),
        "non-Cargo scoped commands must retain the ordinary test path: {scoped}"
    );

    root.write(
        "src/routing.rs",
        "pub fn route_task(value: usize) -> usize { value + 1 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n}\n",
    );
    root.write("tests/connected.rs", "use tiered_alias_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
    let task = store.inspect(task_id).unwrap();
    let mut proof = evidence(&task);
    proof.proof = json!({"command_proof":{"command":"pytest src/routing.rs","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "implement".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert_eq!(store.inspect(task_id).unwrap().stage, "implement");
}

const CANONICAL_FORGE_GITIGNORE: &str = concat!(
    "target/\n",
    "!/.forge/\n",
    "!/.forge/acdd/\n",
    "!/.forge/acdd/**\n",
    "!/.forge/frameworks/\n",
    "!/.forge/frameworks/**\n",
    ".forge/*.sqlite*\n",
    ".forge/*.lock\n",
    ".forge/*.log\n",
    ".forge/*.jsonl\n",
    ".forge/tasks/\n",
    ".forge/checkpoints.json\n",
);

#[test]
fn task_guidance_follows_agent_metadata_workspace_config_and_active_stage() {
    let specification = |agent_type: &str| {
        format!(
        "---\nid: m-guided\ntitle: Guided tasks\ndoc_type: contract\n---\n# Guided tasks\n```yaml\ntask_ref: guided\ntarget: Deliver guidance\nagent_type: {agent_type}\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    )
    };
    let root = ScopedWorkspace::new("forge_guidance_root");
    let linked = ScopedWorkspace::new("forge_guidance_linked");
    let missing = ScopedWorkspace::new("forge_guidance_missing");
    let missing_configured = ScopedWorkspace::new("forge_guidance_missing_configured");
    for (workspace, agent_type) in [
        (&root, "gpt-6-sol"),
        (&linked, "reviewer"),
        (&missing, "flash"),
        (&missing_configured, "worker"),
    ] {
        workspace.write("src/lib.rs", "pub fn guided() {}\n");
        workspace.write("docs/010-guided.md", &specification(agent_type));
    }
    root.write(
        "docs/team-guidance.md",
        "# Root task guidance\nLoad the contextunity-forge skill.\n",
    );
    linked.write(
        "docs/linked-guidance.md",
        "# Linked task guidance\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    linked.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        &format!(
            "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/team-guidance.md\nlinked_workspaces:\n  - name: linked\n    path: {}\n    tasks:\n      enabled: true\n      agents_guidance: docs/linked-guidance.md\n",
            linked.0.display()
        ),
    );
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    linked.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    linked.write(
        "forge-mcp.yaml",
        "task_repository: linked\ntask_project: linked\n",
    );
    missing.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    missing_configured.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/unavailable.md\n",
    );

    for (server, workspace, selector, repository, agent_type, expected_path, absent) in [
        (
            &root.0,
            &root.0,
            None,
            "forge-mcp",
            "gpt-6-sol",
            root.0.join("docs/team-guidance.md"),
            false,
        ),
        (
            &root.0,
            &linked.0,
            Some("linked"),
            "linked",
            "reviewer",
            linked.0.join("docs/linked-guidance.md"),
            false,
        ),
        (
            &missing.0,
            &missing.0,
            None,
            "forge-mcp",
            "flash",
            missing.0.join("AGENTS.md"),
            true,
        ),
        (
            &missing_configured.0,
            &missing_configured.0,
            None,
            "forge-mcp",
            "worker",
            missing_configured.0.join("docs/unavailable.md"),
            true,
        ),
    ] {
        let id = format!("{repository}/m-guided:guided");
        tasks::manage(
            server,
            serde_json::from_value(json!({
                "action": "sync",
                "workspace": selector,
                "milestone_ref": "docs/010-guided.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspect = || {
            tasks::manage(
                server,
                serde_json::from_value(json!({"action": "inspect", "task_id": id})).unwrap(),
            )
            .unwrap()
        };
        let before = inspect();
        assert_eq!(before["spec"]["agent_type"], agent_type);
        assert_eq!(before["agents_guidance"], expected_path.to_str().unwrap());
        let guidance = &before["workflow_guidance"];
        assert!(guidance["rejection"].is_null());
        assert_eq!(guidance["role"], "contract_author");
        assert!(guidance["role_guide"]
            .as_str()
            .unwrap()
            .contains("contract author"));
        assert!(guidance["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        if absent {
            assert_eq!(guidance["warning"]["code"], "TASK_GUIDANCE_MISSING");
        } else {
            assert!(guidance["warning"].is_null());
        }
        let claimed = tasks::claim(
            server,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract".into(),
                worker_id: "guidance-contract-author".into(),
                worktree: workspace.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            claimed["workflow_guidance"]["active_stage"],
            guidance["active_stage"]
        );
        assert_eq!(claimed["workflow_guidance"]["role"], guidance["role"]);
        assert_eq!(claimed["workflow_guidance"]["steps"], guidance["steps"]);
        assert!(claimed["workflow_guidance"]["blackboard_messages"].is_array());
        if repository == "forge-mcp" && !absent {
            let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "contract", &evidence(&task), "pass", None)
                .unwrap();
            let build = inspect();
            assert_eq!(build["workflow_guidance"]["active_stage"], "build");
            assert_eq!(build["workflow_guidance"]["role"], "builder");
            assert_ne!(build["workflow_guidance"]["steps"], guidance["steps"]);
            assert!(build["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.len() >= 2));
            let claimed_build = tasks::claim(
                server,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: "build".into(),
                    worker_id: "guidance-builder".into(),
                    worktree: workspace.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(claimed_build["workflow_guidance"]["role"], "builder");
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "build", &evidence(&task), "pass", None)
                .unwrap();
            let review = inspect();
            assert_eq!(review["workflow_guidance"]["active_stage"], "review");
            assert_eq!(review["workflow_guidance"]["role"], "independent_reviewer");
            assert_eq!(
                review["workflow_guidance"]["independent_from_worker_id"],
                "guidance-builder"
            );
            assert!(review["workflow_guidance"]["independence_rule"]
                .as_str()
                .is_some_and(|rule| rule.contains("different")));

            let claimed_review = tasks::claim(
                server,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: "review".into(),
                    worker_id: "guidance-reviewer".into(),
                    worktree: workspace.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut review_evidence = evidence(&store.inspect(&id).unwrap());
            review_evidence.proof = review_proof("reject");
            let findings =
                json!({"decision":"remediate","summary":"preserve rejected review details"});
            store
                .submit(&id, "review", &review_evidence, "reject", Some(&findings))
                .unwrap();
            let returned = inspect();
            assert_eq!(returned["stage"], "build");
            assert_eq!(
                returned["workflow_guidance"]["rejection"],
                json!({
                    "rejected_from":"review",
                    "worker_id":"guidance-builder",
                    "findings":findings,
                })
            );
            assert!(returned["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.iter().any(|step| {
                    step.as_str()
                        == Some("Return task to builder 'guidance-builder' with review rejection findings to repair defects.")
                })));
            assert_eq!(
                claimed_review["workflow_guidance"]["active_stage"],
                "review"
            );
        }
    }
}

#[test]
fn task_guidance_requires_named_and_installed_forge_skill() {
    let specification = "---\nid: m-skill\ntitle: Skill guidance\ndoc_type: contract\n---\n# Skill guidance\n```yaml\ntask_ref: skill\ntarget: Deliver skill guidance\nagent_type: worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let named = ScopedWorkspace::new("forge_skill_named");
    let named_mcp_skill = ScopedWorkspace::new("forge_skill_named_mcp_skill");
    let unnamed = ScopedWorkspace::new("forge_skill_unnamed");
    let global_only = ScopedWorkspace::new("forge_skill_global_only");
    let ignored_profile = ScopedWorkspace::new("forge_skill_ignored_profile");
    let prefixed_name = ScopedWorkspace::new("forge_skill_prefixed_name");
    let extended_mcp_name = ScopedWorkspace::new("forge_skill_extended_mcp_name");
    let mcp_mention_without_skill_context =
        ScopedWorkspace::new("forge_mcp_mention_without_skill_context");
    let ignored_profile_yaml_exception =
        ScopedWorkspace::new("forge_skill_ignored_profile_yaml_exception");
    let wildcard_yaml_ignore = ScopedWorkspace::new("forge_skill_wildcard_yaml_ignore");
    let wildcard_dot_ignore = ScopedWorkspace::new("forge_skill_wildcard_dot_ignore");
    let runtime_path_negation = ScopedWorkspace::new("forge_skill_runtime_path_negation");
    let markdown_mcp_skill_link = ScopedWorkspace::new("forge_skill_markdown_mcp_link");
    let nested_framework_ignore = ScopedWorkspace::new("forge_skill_nested_framework_ignore");
    let json_configuration_ignore = ScopedWorkspace::new("forge_skill_json_configuration_ignore");
    let runtime_scan_negation = ScopedWorkspace::new("forge_skill_runtime_scan_negation");
    let effective_protection_suffix =
        ScopedWorkspace::new("forge_skill_effective_protection_suffix");
    let unmatched_wildcard_configuration_ignore =
        ScopedWorkspace::new("forge_skill_unmatched_wildcard_configuration_ignore");
    let markdown_heading_reference = ScopedWorkspace::new("forge_skill_markdown_heading_reference");
    let incomplete_gitignore = ScopedWorkspace::new("forge_skill_incomplete_gitignore");
    let missing_gitignore = ScopedWorkspace::new("forge_skill_missing_gitignore");
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &global_only,
        &ignored_profile,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &ignored_profile_yaml_exception,
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &markdown_mcp_skill_link,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
        &markdown_heading_reference,
        &incomplete_gitignore,
        &missing_gitignore,
    ] {
        workspace.write("src/lib.rs", "pub fn skill() {}\n");
        workspace.write("docs/010-skill.md", specification);
        workspace.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    }
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &incomplete_gitignore,
        &missing_gitignore,
        &ignored_profile_yaml_exception,
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &markdown_mcp_skill_link,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
        &markdown_heading_reference,
    ] {
        workspace.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
    }
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &global_only,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &markdown_mcp_skill_link,
        &markdown_heading_reference,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
    ] {
        workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    }
    named.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    named_mcp_skill.write(
        "AGENTS.md",
        "# Rules\nLoad contextunity-forge-mcp skill for ACDD.\n",
    );
    unnamed.write(
        "AGENTS.md",
        "# Rules\nUse contextunity-forge-mcp for queries.\n",
    );
    prefixed_name.write("AGENTS.md", "# Rules\nLoad xcontextunity-forge skill.\n");
    extended_mcp_name.write(
        "AGENTS.md",
        "# Rules\nLoad contextunity-forge-mcp-tools skill for ACDD.\n",
    );
    mcp_mention_without_skill_context.write(
        "AGENTS.md",
        "# Rules\nUse contextunity-forge-mcp for queries.\nThe worker skill is installed.\n",
    );
    markdown_mcp_skill_link.write(
        "AGENTS.md",
        "# Rules\nLoad [contextunity-forge-mcp](https://example.com/SKILL.md) skill for ACDD.\n",
    );
    markdown_heading_reference.write("AGENTS.md", "# Intro\n\ncontextunity-forge skill.\n");
    for workspace in [
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
    ] {
        workspace.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    }
    global_only.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    ignored_profile.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    ignored_profile.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    ignored_profile.write(".gitignore", "target/\n.forge/\n");
    ignored_profile_yaml_exception
        .write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    wildcard_yaml_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}*.yaml\n"),
    );
    wildcard_dot_ignore.write(".gitignore", &format!("{CANONICAL_FORGE_GITIGNORE}.*\n"));
    runtime_path_negation.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}!.forge/tasks.sqlite\n"),
    );
    nested_framework_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}.forge/frameworks/nested/\n"),
    );
    json_configuration_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}*.json\n"),
    );
    runtime_scan_negation.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}!.forge/scan.sqlite\n"),
    );
    effective_protection_suffix.write(
        ".gitignore",
        &format!(
            ".*\n*.json\n.forge/frameworks/nested/\n!.forge/scan.sqlite\n{CANONICAL_FORGE_GITIGNORE}"
        ),
    );
    // This active wildcard misses the validator's representative paths; the
    // final-suffix invariant must reject it without enumerating file types.
    unmatched_wildcard_configuration_ignore
        .write(".gitignore", &format!("{CANONICAL_FORGE_GITIGNORE}*.md\n"));
    ignored_profile_yaml_exception.write(".gitignore", "target/\n.forge/\n!.forge/*.yaml\n");
    markdown_heading_reference.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    incomplete_gitignore.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    incomplete_gitignore.write(".gitignore", "target/\n");
    missing_gitignore.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    let global_installed = std::env::var_os("HOME").is_some_and(|home| {
        std::path::Path::new(&home)
            .join(".agents/skills/contextunity-forge/SKILL.md")
            .is_file()
    });
    for (workspace, warning_fragment) in [
        (&named, None),
        (&named_mcp_skill, None),
        (&markdown_mcp_skill_link, None),
        (&markdown_heading_reference, None),
        (&effective_protection_suffix, None),
        (
            &unmatched_wildcard_configuration_ignore,
            Some("Edit `.gitignore`"),
        ),
        (&unnamed, Some("do not name the contextunity-forge skill")),
        (
            &prefixed_name,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            &extended_mcp_name,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            &mcp_mention_without_skill_context,
            Some("do not name the contextunity-forge skill"),
        ),
        (&global_only, (!global_installed).then_some("not installed")),
        (&ignored_profile, Some("Edit `.gitignore`")),
        (&ignored_profile_yaml_exception, Some("Edit `.gitignore`")),
        (&wildcard_yaml_ignore, Some("Edit `.gitignore`")),
        (&wildcard_dot_ignore, Some("Edit `.gitignore`")),
        (&runtime_path_negation, Some("Edit `.gitignore`")),
        (&nested_framework_ignore, Some("Edit `.gitignore`")),
        (&json_configuration_ignore, Some("Edit `.gitignore`")),
        (&runtime_scan_negation, Some("Edit `.gitignore`")),
        (&incomplete_gitignore, Some("Edit `.gitignore`")),
        (&missing_gitignore, Some("Edit `.gitignore`")),
    ] {
        tasks::manage(
            &workspace.0,
            serde_json::from_value(json!({
                "action": "sync",
                "milestone_ref": "docs/010-skill.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspected = tasks::manage(
            &workspace.0,
            serde_json::from_value(json!({
                "action": "inspect",
                "task_id": "forge-mcp/m-skill:skill"
            }))
            .unwrap(),
        )
        .unwrap();
        let warning = &inspected["workflow_guidance"]["warning"];
        match warning_fragment {
            None => assert!(warning.is_null()),
            Some(fragment) => {
                assert_eq!(warning["code"], "TASK_GUIDANCE_MISSING");
                let message = warning["message"].as_str().unwrap();
                assert!(message.contains(fragment), "{message}");
                if fragment == "Edit `.gitignore`" {
                    assert!(warning["path"]
                        .as_str()
                        .is_some_and(|path| path.ends_with(".gitignore")));
                    for rule in [
                        "!/.forge/",
                        "!/.forge/acdd/",
                        "!/.forge/acdd/**",
                        "!/.forge/frameworks/",
                        "!/.forge/frameworks/**",
                        ".forge/*.sqlite*",
                        ".forge/*.lock",
                        ".forge/*.log",
                        ".forge/*.jsonl",
                        ".forge/tasks/",
                        ".forge/checkpoints.json",
                    ] {
                        assert!(message.contains(rule), "missing `{rule}` in: {message}");
                    }
                }
            }
        }
    }
}

#[test]
fn task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite() {
    let schema = serde_json::to_value(schemars::schema_for!(tasks::Submit)).unwrap();
    assert_eq!(schema["properties"]["evidence"]["type"], "object");
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contract = store
        .claim(&task_id, "contract", "contract-author", worktree)
        .unwrap();
    let mut proof = evidence(&contract);
    proof.commit = None;
    let raw_evidence = serde_json::to_value(&proof).unwrap();
    for invalid in [
        json!({"task_id":task_id,"stage":"contract","action":"pass"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence_ref":"proof.yaml"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence":raw_evidence,"evidence_ref":"proof.yaml"}),
    ] {
        assert!(serde_json::from_value::<tasks::Submit>(invalid).is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let nonobject: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": 42
    }))
    .unwrap();
    assert!(tasks::submit(&root.0, nonobject).is_err());
    assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    let request: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": raw_evidence
    }))
    .unwrap();

    let submitted = tasks::submit(&root.0, request).unwrap();
    assert_eq!(submitted["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    let mut expected = raw_evidence.clone();
    expected["commit"] = stored["commit"].clone();
    expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
    assert_eq!(stored, expected);
    let snapshot_commit = stored["commit"].as_str().unwrap();
    assert_eq!(snapshot_commit.len(), 40);
    assert_eq!(snapshot_commit, submitted["snapshot"]["commit"]);
    assert_eq!(stored["candidate_baseline_head"], git_head(&root.0));
    let submission: String = store
        .connection
        .query_row(
            "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
            rusqlite::params![task_id, proof.claim_revision],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"],
        stored
    );
}

#[test]
fn task_cli_submit_accepts_json_object_and_persists_gate_evidence() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let claimed = store
        .claim(&task_id, "contract", "cli-worker", root.0.to_str().unwrap())
        .unwrap();
    let proof = evidence(&claimed);
    let raw = serde_json::to_string(&proof).unwrap();
    for invalid in ["proof.yaml", "42"] {
        let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "submit",
                &task_id,
                "--stage",
                "contract",
                "--action",
                "pass",
                "--evidence",
                invalid,
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            &task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            &raw,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    let mut expected = serde_json::to_value(proof).unwrap();
    expected["commit"] = stored["commit"].clone();
    expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
    assert_eq!(stored, expected);
    let snapshot_commit = stored["commit"].as_str().unwrap();
    assert_eq!(snapshot_commit.len(), 40);
    assert_eq!(snapshot_commit, result["snapshot"]["commit"]);
    assert_eq!(stored["candidate_baseline_head"], git_head(&root.0));
}

#[test]
fn typed_task_proofs_pass_through_engine_and_real_store() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable": true, "evidence": "verified at the public seam"}),
                )
            })
            .collect();
    let cases = [
        (
            "contract",
            "contract-reviewer",
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}}),
            vec![
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":0}}),
                json!({"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}),
                json!({"contract_proof":{"seam_test":"tests/acdd/tasks.rs","red_exit_code":101}}),
            ],
        ),
        (
            "build",
            "builder",
            json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"running 2 tests\ntest result: ok. 2 passed ✓"}}),
            vec![
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":1,"tests_passed":1,"tests_failed":1}}),
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"x".repeat(65_537)}}),
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0}}),
                json!({"command":"cargo test --test acdd","result":"passed","artifacts":[]}),
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"output":"passed"}}),
            ],
        ),
        (
            "review",
            "independent-reviewer",
            json!({"review_proof":{"decision":"pass","contours":contours.clone()}}),
            vec![
                json!({"review_proof":{"decision":"pass","contours":{}}}),
                json!({"decision":"pass","contours":contours}),
            ],
        ),
    ];
    for (stage, worker, proof, invalid_cases) in cases {
        let claimed = store.claim(&task_id, stage, worker, worktree).unwrap();
        let mut evidence = evidence(&claimed);
        for invalid in invalid_cases {
            evidence.proof = invalid;
            let request: tasks::Submit = serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": evidence,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, request).is_err());
            assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        }
        evidence.proof = proof.clone();
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id": task_id,
            "stage": stage,
            "action": "pass",
            "evidence": evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(result["status"], "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed'",
                rusqlite::params![task_id, GATES[claimed.gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            proof
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            proof
        );
    }
}

#[test]
fn rejected_task_gates_require_typed_proof_and_persist_failing_evidence() {
    for gate in [0_usize, 1, 2] {
        let (root, mut store, milestone) = fixture();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let worktree = root.0.to_str().unwrap();
        for stage in GATES.iter().take(gate) {
            let claimed = store.claim(&task_id, stage, "builder", worktree).unwrap();
            store
                .submit(&task_id, stage, &evidence(&claimed), "pass", None)
                .unwrap();
        }
        let worker = if gate == 2 { "reviewer" } else { "builder" };
        let claimed = store
            .claim(&task_id, GATES[gate], worker, worktree)
            .unwrap();
        let (bare, typed) = match gate {
            0 => (
                json!({"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}),
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}}),
            ),
            1 => (
                json!({"command":"cargo test","result":"failed","artifacts":[]}),
                json!({"command_proof":{"command":"cargo test","exit_code":101,"tests_passed":0,"tests_failed":1,"log":"one test failed"}}),
            ),
            _ => (
                json!({"decision":"reject","contours":{}}),
                review_proof("reject"),
            ),
        };
        let findings = json!({"decision":"remediate"});
        let mut proof = evidence(&claimed);
        proof.proof = bare;
        assert!(store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        if gate == 2 {
            for invalid in [
                json!({"review_proof":{"decision":"reject","contours":{}}}),
                passing_review_proof(),
            ] {
                proof.proof = invalid;
                assert!(store
                    .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
                    .is_err());
                assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
            }
        }
        proof.proof = typed.clone();
        let submitted = store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .unwrap();
        assert_eq!(submitted.status, "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='rejected'",
                rusqlite::params![task_id, GATES[gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            typed
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            typed
        );
    }
}

#[test]
fn linked_task_workspaces_share_storage_and_confine_local_scope() {
    let (root, _, _) = fixture();
    let linked = ScopedWorkspace::new("forge_linked_tasks");
    let missing = ScopedWorkspace::new("forge_missing_milestones");
    let empty = ScopedWorkspace::new("forge_empty_milestones");
    empty.write("docs/milestones/README.md", "# No task manifests\n");
    linked.write("src/lib.rs", "pub fn linked() {}\n");
    linked.write("docs/AGENTS.md", "# Linked repository rules\n");
    linked.write(
        "forge-mcp.yaml",
        "task_repository: traverse-library\ntask_project: tooling\n",
    );
    linked.write(
        "contracts/010-linked.md",
        &SPEC
            .replace("id: m-test", "id: m-linked")
            .replace("isolated", "linked-rules"),
    );
    root.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: docs/AGENTS.md\n  - name: missing\n    path: {}\n    tasks: {{enabled: true}}\n  - name: empty\n    path: {}\n    tasks: {{enabled: true}}\n  - name: disabled\n    path: {}\n    tasks: {{enabled: false}}\n  - name: index-only\n    path: {}\n",linked.0.display(),missing.0.display(),empty.0.display(),linked.0.display(),linked.0.display()));
    let sync = |workspace: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","workspace":workspace})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(sync("missing")["tasks"], json!([]));
    assert_eq!(sync("empty")["tasks"], json!([]));
    let imported = sync("traverse");
    assert_eq!(imported["tasks"].as_array().unwrap().len(), 2);
    let id = imported["tasks"][0]["task_id"].as_str().unwrap();
    assert!(id.starts_with("tooling/"));
    let list =
        |arguments| tasks::list(&root.0, serde_json::from_value(arguments).unwrap()).unwrap();
    let linked_by_milestone = list(json!({"milestone_ref":"m-linked","status":"all"}));
    let linked_by_milestone_tasks = linked_by_milestone["tasks"].as_array().unwrap();
    assert_eq!(linked_by_milestone_tasks.len(), 2);
    assert!(linked_by_milestone_tasks
        .iter()
        .all(|task| { task["task_id"].as_str().unwrap().starts_with("tooling/") }));
    assert_eq!(list(json!({}))["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        list(json!({"repository":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        list(json!({"repository":"traverse"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list(json!({"repository":"all","status":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    linked.write(
        "contracts/020-linked.md",
        &SPEC.replace("m-test", "m-linked-020"),
    );
    root.write(
        "docs/milestones/020-primary-alpha.md",
        &SPEC.replace("m-test", "m-primary-alpha"),
    );
    root.write(
        "docs/milestones/020-primary-beta.md",
        &SPEC.replace("m-test", "m-primary-beta"),
    );
    let ambiguous_primary = tasks::list(
        &root.0,
        serde_json::from_value(json!({"milestone_ref":"020","status":"all"})).unwrap(),
    )
    .unwrap_err();
    assert!(
        ambiguous_primary
            .to_string()
            .contains("multiple active documents"),
        "a primary ambiguity must not fall through to a unique linked match: {ambiguous_primary}"
    );
    let inspect = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"inspect","task_id":id})).unwrap(),
    )
    .unwrap();
    assert_eq!(inspect["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(
        inspect["agents_guidance"],
        linked.0.join("docs/AGENTS.md").to_str().unwrap()
    );
    assert_eq!(inspect["applicable_invariants"], json!(["linked-rules"]));
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    assert!(extend("src/new.rs").is_ok());
    std::os::unix::fs::symlink(root.0.join("src"), linked.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/main.rs").is_err());
    assert!(extend("../outside.rs").is_err());
    let claim = |worktree: &std::path::Path| {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.into(),
                stage: "contract".into(),
                worker_id: "linked-builder".into(),
                worktree: worktree.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
    };
    assert!(claim(&root.0).is_err());
    let claimed = claim(&linked.0).unwrap();
    assert_eq!(claimed["workspace_root"], inspect["workspace_root"]);
    assert_eq!(claimed["agents_guidance"], inspect["agents_guidance"]);
    let connection = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let namespaces: i64 = connection
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(namespaces, 4);
}
#[test]
fn linked_scope_extension_validates_owner_and_claimed_worktree() {
    let (root, _, _) = fixture();
    let owner = ScopedWorkspace::new("forge_scope_owner");
    let worktree = ScopedWorkspace::new("forge_scope_worktree");
    for workspace in [&owner, &worktree] {
        workspace.write("src/lib.rs", "pub fn local() {}\n");
        workspace.write("docs/010-test.md", SPEC);
    }
    root.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n",
            owner.0.display()
        ),
    );
    let synced = tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: worktree.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    std::os::unix::fs::symlink(root.0.join("src"), owner.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    std::fs::remove_file(owner.0.join("src/foreign")).unwrap();
    std::os::unix::fs::symlink(root.0.join("src"), worktree.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    assert!(extend("src/local.rs").is_ok());
}
#[test]
fn nested_repository_tasks_preserve_distinct_write_perimeters() {
    let main = ScopedWorkspace::new("forge_nested_repositories");
    main.write("src/main.rs", "pub fn main_owner() {}\n");
    main.write(
        "docs/010-main.md",
        &SPEC.replace("scope: [src/]", "scope: [src/main.rs]"),
    );
    main.write("src/library/src/lib.rs", "pub fn library_owner() {}\n");
    main.write("src/library/docs/milestones/010-lib.md", SPEC);
    main.write("forge-mcp.yaml", "linked_workspaces:\n  - name: library\n    path: src/library\n    tasks: {enabled: true}\n");
    let imported = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-main.md"}))
            .unwrap(),
    )
    .unwrap();
    let primary_id = imported["tasks"][0]["task_id"].as_str().unwrap();
    let imported_linked = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","workspace":"library"})).unwrap(),
    )
    .unwrap();
    let library_id = imported_linked["tasks"][0]["task_id"].as_str().unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &main.0,
            serde_json::from_value(
                json!({"action":"extend_scope","task_id":primary_id,"paths":[path]}),
            )
            .unwrap(),
        )
    };
    assert!(extend("src/library/src/foreign.rs").is_err());
    assert!(extend("src/main_helpers.rs").is_ok());
    let claimed = tasks::claim(
        &main.0,
        tasks::Claim {
            task_id: library_id.into(),
            stage: "contract".into(),
            worker_id: "library-builder".into(),
            worktree: main.0.join("src/library").to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        claimed["workspace_root"],
        main.0.join("src/library").to_str().unwrap()
    );
    main.write(
        "docs/010-wide.md",
        &SPEC.replace("id: m-test", "id: m-wide"),
    );
    assert!(tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-wide.md"}))
            .unwrap()
    )
    .is_err());
}
#[test]
fn task_specification_and_store_survive_code_index_rebuild() {
    let (root, mut store, milestone) = fixture();
    let first = &milestone.tasks[0];
    let digest = milestone.digest(first).unwrap();
    let mut changed = milestone.clone();
    changed.tasks[1].target = "Sibling amendment".into();
    assert_eq!(digest, changed.digest(first).unwrap());
    let id = milestone.task_id(first);
    let claim = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    drop(store);
    let store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        store.inspect(&id).unwrap().claim_revision,
        claim.claim_revision
    );
    assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
    let wal: String = store
        .connection
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wal, "wal");
    let timeout: i64 = store
        .connection
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
}
#[test]
fn task_configuration_defaults_and_crlf_preserve_operational_identity() {
    let (root, _, milestone) = fixture();
    root.write("forge-mcp.yaml", "roots: [src]\n");
    assert_eq!(
        tasks::database_path(&root.0).unwrap(),
        root.0.join(".forge/tasks.sqlite")
    );
    let crlf = Milestone::parse(&SPEC.replace('\n', "\r\n"), "forge-mcp").unwrap();
    assert_eq!(
        crlf.task_id(&crlf.tasks[0]),
        milestone.task_id(&milestone.tasks[0])
    );
    assert_eq!(
        crlf.digest(&crlf.tasks[0]).unwrap(),
        milestone.digest(&milestone.tasks[0]).unwrap()
    );
    let id = milestone.task_id(&milestone.tasks[0]);
    let missing = root.0.join("not-created");
    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: missing.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("WORKTREE_NOT_FOUND"));
    let task = tasks::store(&root.0).unwrap().inspect(&id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());
}
#[test]
fn linked_worktree_resolves_tasks_db_to_primary_worktree_root() {
    let primary = ScopedWorkspace::new("forge_primary_git");
    let wt = ScopedWorkspace::new("forge_wt_git");
    let run = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    run(&primary.0, &["init", "-b", "main"]);
    run(&primary.0, &["config", "user.name", "Test User"]);
    run(&primary.0, &["config", "user.email", "test@example.com"]);
    primary.write("README.md", "# test\n");
    primary.write("forge-mcp.yaml", "roots: [src]\n");
    run(&primary.0, &["add", "."]);
    run(&primary.0, &["commit", "-m", "initial"]);
    let wt_out = run(
        &primary.0,
        &["worktree", "add", wt.0.to_str().unwrap(), "-b", "test-wt"],
    );
    if wt_out.status.success() {
        let expected = primary
            .0
            .canonicalize()
            .unwrap()
            .join(".forge/tasks.sqlite");
        let resolved = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved, expected);
        // Fallback when forge-mcp.yaml is absent in worktree:
        let _ = std::fs::remove_file(wt.0.join("forge-mcp.yaml"));
        let resolved_fallback = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved_fallback, expected);
        let _ = run(
            &primary.0,
            &["worktree", "remove", "--force", wt.0.to_str().unwrap()],
        );
    }
}
#[test]
fn tasks_coordinate_claims_dependencies_reset_scope_and_claim_diagnostic() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let listed = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: None,
            milestone_status: None,
            status: None,
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed["tasks"][0]["task_id"], id);
    let path = root.0.join(".forge/tasks.sqlite");
    let worktree = root.0.to_str().unwrap().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let path = path.clone();
            let worktree = worktree.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = TasksStore::open(&path).unwrap();
                barrier.wait();
                store.claim(&id, "contract", &format!("worker-{n}"), &worktree)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let winner = store.inspect(&id).unwrap();
    let claimed_at: i64 = store
        .connection
        .query_row(
            "SELECT claimed_at FROM task_claims WHERE task_id=?1 AND ended=0",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    let winner_id = winner.worker_id.as_deref().unwrap();
    let expected_collision = format!(
        "TASK_ALREADY_CLAIMED: {id}. Claimed by worker '{winner_id}' in worktree '{}' (revision {}, claimed at {claimed_at}). Do: submit the active claim from worker '{winner_id}' before opening another.",
        winner.worktree.as_deref().unwrap(),
        winner.claim_revision,
    );
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.to_string() == expected_collision));
    assert!(milestone.acdd_profile.is_none());
    assert!(milestone.tasks[0].acdd_profile.is_none());
    let before_sync = store.inspect(&id).unwrap();
    assert_eq!(before_sync.profile_pin, None);
    let synced = store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    assert_eq!(synced.len(), 2);
    let after_sync = store.inspect(&id).unwrap();
    assert_eq!(after_sync.profile_pin, None);
    assert_eq!(after_sync.digest, before_sync.digest);
    assert_eq!(after_sync.status, before_sync.status);
    assert_eq!(after_sync.stage, before_sync.stage);
    assert_eq!(after_sync.claim_revision, before_sync.claim_revision);
    let before = store.inspect(&id).unwrap();
    let reset = store.reset(&id).unwrap();
    assert!(reset.claim_revision > before.claim_revision);
    assert_eq!(reset.status, "ready");
    let extended = store
        .extend_scope(
            &id,
            &["src/next.rs".into(), "tests/fixture.rs".into()],
            &root.0,
        )
        .unwrap();
    assert!(extended.spec.scope.contains(&"src/next.rs".into()));
    assert!(extended.spec.scope.contains(&"tests/fixture.rs".into()));
    for path in ["../escape", "docs/outside.rs"] {
        assert!(store.extend_scope(&id, &[path.into()], &root.0).is_err());
    }
    let external = ScopedWorkspace::new("forge_scope_external");
    std::os::unix::fs::symlink(&external.0, root.0.join("src/escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/escape/file.rs".into()], &root.0)
        .is_err());
    std::os::unix::fs::symlink(root.0.join("docs"), root.0.join("src/module_escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/module_escape/file.rs".into()], &root.0)
        .is_err());
}
#[test]
fn gates_write_receipts_and_preserve_retention_outcomes() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    store
        .blackboard_post(
            &id,
            "builder",
            "decisions",
            "Keep the parser boundary stable.",
        )
        .unwrap();
    store
        .blackboard_post(
            &id,
            "builder",
            "deferred",
            "Measure large fixture cost later.",
        )
        .unwrap();
    store
        .connection
        .execute(
            "UPDATE task_blackboard SET created_at=1 WHERE task_id=?1",
            [&id],
        )
        .unwrap();
    for (gate, stage) in GATES.iter().enumerate() {
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = store.inspect(&id).unwrap();
        let mut evidence = evidence(&task);
        evidence.commit = None;
        if gate == 3 {
            evidence.proof = json!({"stage":stage});
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":id,
                "stage":stage,
                "action":"pass",
                "evidence":evidence,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
            evidence.proof = json!({"delivery_proof":{}});
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":id,
            "stage":stage,
            "action":"pass",
            "evidence":evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(
            result["status"],
            if gate == 3 { "completed" } else { "ready" }
        );
    }
    let completed = store.inspect(&id).unwrap();
    let rollup = completed.receipt.as_ref().unwrap().rollup.as_ref().unwrap();
    assert_eq!(
        rollup.architectural_notes,
        vec![
            "decisions: Keep the parser boundary stable.",
            "deferred: Measure large fixture cost later."
        ]
    );
    assert!(completed.receipt.as_ref().unwrap().commit.is_some());
    let remaining_messages: i64 = store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_blackboard WHERE task_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining_messages, 0);
    let at = completed.completed_at.unwrap();
    assert!(store
        .delete(Some(&id), None, false, at + RETENTION_SECONDS)
        .is_err());
    assert_eq!(store.cleanup(at + RETENTION_SECONDS + 1).unwrap(), vec![id]);
    assert_eq!(store.list(None, "ready", None).unwrap().len(), 1);
    let rows: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM task_claims", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
#[test]
fn specifications_validate_unique_identities_and_governing_digests() {
    let parsed = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let first = &parsed.tasks[0];
    let digest = parsed.digest(first).unwrap();
    let mut completed = first.clone();
    completed.status = Some("completed".into());
    completed.receipt = Some(Receipt {
        commit: Some("candidate".into()),
        contract_revision: 1,
        passed_at: "timestamp".into(),
        evidence: json!({}),
        review: json!({}),
        decision: "pass".into(),
        rollup: None,
    });
    assert_eq!(parsed.digest(&completed).unwrap(), digest);
    let mut constrained = parsed.clone();
    constrained.invariants.push("WAL coordination".into());
    assert_ne!(constrained.digest(first).unwrap(), digest);
    for malformed in [
        SPEC.replace("task_ref: second", "task_ref: first"),
        SPEC.replace("depends_on: [first]", "depends_on: [missing]"),
        SPEC.replace("scope: [src/]", "scope: [../outside]"),
        SPEC.replace("id: m-test", "id: []"),
    ] {
        assert!(Milestone::parse(&malformed, "forge-mcp").is_err());
    }
}
#[test]
fn claim_reports_structured_authority_gap_for_frontmatter_and_task_drift() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let changed = SPEC
        .replace(
            "status: active\n",
            "status: active\nowners: [docs/AGENTS.md]\n",
        )
        .replace("target: Deliver first", "target: Deliver amended first");
    root.write("docs/010-test.md", &changed);

    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();

    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; frontmatter fields [milestone.owners], task fields [task.target]. Do: revert [milestone.owners] in docs/010-test.md, set contract_revision above 1, and run contextunity-forge-mcp task sync."
        )
    );
}

#[test]
fn authority_gap_attributes_milestone_invariants_and_owners_with_specific_repair() {
    for (expected_fields, advice) in [
        (
            "milestone.invariants",
            ". Declare task invariants inside task.invariants",
        ),
        ("milestone.owners", ""),
    ] {
        let (root, _, milestone) = fixture();
        let id = milestone.task_id(&milestone.tasks[0]);
        let changed = if expected_fields == "milestone.invariants" {
            SPEC.replace("invariants: [isolated]", "invariants: [changed]")
        } else {
            SPEC.replace(
                "status: active\n",
                "status: active\nowners: [docs/AGENTS.md]\n",
            )
        };
        root.write("docs/010-test.md", &changed);
        let error = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract".into(),
                worker_id: "header-authority-check".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            error,
            format!(
                "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; digest fields [{expected_fields}]. Do: revert [{expected_fields}] in docs/010-test.md{advice}."
            )
        );
    }
}

#[test]
fn sync_reports_structured_authority_gap_for_task_drift_at_equal_revision() {
    let (root, mut store, milestone) = fixture();
    let changed_text = SPEC.replace("target: Deliver first", "target: Deliver changed");
    let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
    let error = store
        .sync(&changed, "docs/010-test.md", &root.0)
        .unwrap_err()
        .to_string();
    let id = milestone.task_id(&milestone.tasks[0]);
    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: task '{id}'. file revision 1 <= stored 1; digest fields [task.target]. Do: set contract_revision above 1 and run contextunity-forge-mcp task sync."
        )
    );
}

#[test]
fn milestone_id_change_is_rejected_before_sync_admission_and_claim_lookup() {
    let (root, mut store, milestone) = fixture();
    let original_id = milestone.task_id(&milestone.tasks[0]);
    let changed_text = SPEC.replace("id: m-test", "id: m-renamed");
    root.write("docs/010-test.md", &changed_text);
    let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
    let sync_error = store
        .sync(&changed, "docs/010-test.md", &root.0)
        .unwrap_err()
        .to_string();
    assert_eq!(
        sync_error,
        "AUTHORITY_GAP: milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; sync would admit them as new tasks."
    );
    assert!(store.inspect("forge-mcp/m-renamed:first").is_err());

    let claim_error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: original_id,
            stage: "contract".into(),
            worker_id: "id-change-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        claim_error,
        "AUTHORITY_GAP: forge-mcp/m-test:first. milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; task cannot be resolved."
    );
}

#[test]
fn milestone_id_guard_ignores_foreign_project_at_the_same_manifest_path() {
    let root = ScopedWorkspace::new("forge_tasks_shared_manifest_path");
    let database = root.0.join(".forge/tasks.sqlite");
    let mut foreign_store = TasksStore::open_project(&database, "forge-mcp", "aaa").unwrap();
    let mut current_store = TasksStore::open_project(&database, "forge-mcp", "zzz").unwrap();
    let specification = |project: &str, id: &str| {
        format!(
            "---\nrepository: forge-mcp\nproject: {project}\nid: {id}\ntitle: Shared path\ndoc_type: contract\nstatus: active\n---\n# Shared path\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        )
    };
    let foreign = Milestone::parse(&specification("aaa", "m-test"), "forge-mcp").unwrap();
    foreign_store
        .sync(&foreign, "docs/010-shared.md", &root.0)
        .unwrap();
    let current = Milestone::parse(&specification("zzz", "m-test"), "forge-mcp").unwrap();
    current_store
        .sync(&current, "docs/010-shared.md", &root.0)
        .unwrap();

    let renamed = Milestone::parse(&specification("zzz", "m-renamed"), "forge-mcp").unwrap();
    let error = current_store
        .sync(&renamed, "docs/010-shared.md", &root.0)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "AUTHORITY_GAP: milestone 'docs/010-shared.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-shared.md. Task ids are zzz/m-renamed:first; sync would admit them as new tasks."
    );
    assert!(current_store.inspect("zzz/m-renamed:first").is_err());
}

#[test]
fn claim_distinguishes_missing_milestone_file_from_missing_task() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    std::fs::remove_file(root.0.join("docs/010-test.md")).unwrap();
    let missing_file = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "missing-file-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        missing_file,
        format!(
            "TASK_AUTHORITY_MISSING: {id}. milestone file 'docs/010-test.md' is missing. Do: restore milestone file 'docs/010-test.md'."
        )
    );

    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    root.write(
        "docs/010-test.md",
        &SPEC
            .replace("task_ref: first", "task_ref: renamed")
            .replace("depends_on: [first]", "depends_on: [renamed]"),
    );
    let missing_task = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "missing-task-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        missing_task,
        format!(
            "TASK_AUTHORITY_MISSING: {id}. task '{id}' not found in milestone document 'docs/010-test.md'. Do: restore task '{id}' in 'docs/010-test.md'."
        )
    );
}

#[test]
fn unsynced_higher_revision_with_header_drift_reports_header_and_sync_action() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let changed = SPEC
        .replace(
            "status: active\n",
            "status: active\nowners: [docs/AGENTS.md]\n",
        )
        .replace("task_ref: first", "task_ref: first\ncontract_revision: 2");
    root.write("docs/010-test.md", &changed);
    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "unsynced-revision-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: {id}. task '{id}'. file revision 2 > stored 1 (unsynced); digest fields [milestone.owners]. Do: revert [milestone.owners] in docs/010-test.md and run contextunity-forge-mcp task sync."
        )
    );
    assert!(!error.contains("task.contract_revision"));
}

#[test]
fn contract_readmission_and_reset_fence_old_evidence() {
    let (root, mut store, mut milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    let stale = evidence(&first);
    let reset = store.reset(&id).unwrap();
    assert!(store.submit(&id, "contract", &stale, "pass", None).is_err());
    let second = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    assert!(second.claim_revision > reset.claim_revision);
    milestone.tasks[0].target = "Amended target".into();
    assert!(store.sync(&milestone, "docs/010-test.md", &root.0).is_err());
    assert_eq!(
        store.inspect(&id).unwrap().worker_id.as_deref(),
        Some("builder")
    );
    milestone.tasks[0].contract_revision = 2;
    let synced = store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    assert_eq!(synced[0].contract_revision, 2);
    assert_eq!(synced[0].gate, 0);
    assert!(synced[0].claim_revision > second.claim_revision);
    assert!(store
        .submit(&id, "contract", &evidence(&second), "pass", None)
        .is_err());
}
#[test]
fn review_rejection_retains_findings_and_requires_a_new_build() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    for stage in ["contract", "build"] {
        let claimed = store
            .claim(&id, stage, "builder", root.0.to_str().unwrap())
            .unwrap();
        store
            .submit(&id, stage, &evidence(&claimed), "pass", None)
            .unwrap();
    }
    let claimed = store
        .claim(&id, "review", "builder", root.0.to_str().unwrap())
        .unwrap();
    let mut review = evidence(&claimed);
    review.proof = passing_review_proof();
    assert!(store.submit(&id, "review", &review, "pass", None).is_err());
    store.reset(&id).unwrap();
    let claimed = store
        .claim(&id, "review", "reviewer", root.0.to_str().unwrap())
        .unwrap();
    let findings = json!({"paths":["src/lib.rs"],"decision":"remediate"});
    let mut review = evidence(&claimed);
    review.proof = review_proof("reject");
    let rejected = store
        .submit(&id, "review", &review, "reject", Some(&findings))
        .unwrap();
    assert_eq!(rejected.gate, 1);
    assert_eq!(rejected.status, "ready");
    assert_eq!(
        store.inspect_details(&id).unwrap()["findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let rebuilt = store
        .claim(&id, "build", "builder", root.0.to_str().unwrap())
        .unwrap();
    store
        .submit(&id, "build", &evidence(&rebuilt), "pass", None)
        .unwrap();
    assert_eq!(store.inspect(&id).unwrap().gate, 2);
}
#[test]
fn force_deletion_preserves_project_boundaries_and_revision_fencing() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "worker", root.0.to_str().unwrap())
        .unwrap();
    assert!(store
        .delete(None, Some("docs/010-test.md"), false, 0)
        .is_err());
    assert_eq!(store.list(None, "all", None).unwrap().len(), 2);
    let deleted = store.delete(Some(&id), None, true, 0).unwrap();
    assert_eq!(deleted.revoked_claims, 1);
    assert_eq!(deleted.count, 1);
    assert!(store.list(None, "ready", None).unwrap().is_empty());
    let recreated = store
        .create(&milestone, "docs/010-test.md", &root.0, "first")
        .unwrap();
    assert!(recreated[0].claim_revision > first.claim_revision);
    let other = contextunity_forge_mcp::db::tasks_store::TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "other",
        "project",
    )
    .unwrap();
    assert!(other.list(None, "all", None).unwrap().is_empty());
    assert!(other.inspect(&id).is_err());
}
#[test]
fn dedicated_store_rejects_index_and_unknown_schema_without_ddl() {
    let root = ScopedWorkspace::new("forge_task_schema");
    root.write("src/lib.rs", "pub fn sample() {}\n");
    root.write("forge-mcp.yaml", "roots: [src]\ndocs: []\n");
    let index = root.0.join("index.sqlite");
    writer::build(&root.0, &index, None).unwrap();
    assert!(TasksStore::open(&index).is_err());
    let conn = rusqlite::Connection::open(&index).unwrap();
    let task_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='tasks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_tables, 0);
    for version in ["1", "2", "3", "4", "99"] {
        let old = root.0.join(format!("schema-{version}.sqlite"));
        let conn = rusqlite::Connection::open(&old).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY,value TEXT);\
             INSERT INTO task_store_metadata VALUES('schema_version','{version}');\
             CREATE TABLE sentinel(value TEXT NOT NULL);\
             INSERT INTO sentinel VALUES('preserve me');"
        ))
        .unwrap();
        assert!(
            TasksStore::open(&old).is_err(),
            "schema {version} must fail closed"
        );
        let stored_version: String = conn
            .query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_version, version, "schema marker {version} changed");
        let sentinel: String = conn
            .query_row("SELECT value FROM sentinel", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sentinel, "preserve me");
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(tables, ["sentinel", "task_store_metadata"]);
    }

    let unversioned = root.0.join("unversioned.sqlite");
    let conn = rusqlite::Connection::open(&unversioned).unwrap();
    conn.execute_batch(
        "CREATE TABLE old_data(value TEXT); INSERT INTO old_data VALUES('preserve me');",
    )
    .unwrap();
    assert!(TasksStore::open(&unversioned).is_err());
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        tables, 1,
        "unversioned non-empty DB must not be initialized"
    );
    let value: String = conn
        .query_row("SELECT value FROM old_data", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "preserve me");

    let unversioned_view = root.0.join("unversioned-view.sqlite");
    let conn = rusqlite::Connection::open(&unversioned_view).unwrap();
    conn.execute_batch("CREATE VIEW old_projection AS SELECT 1 AS value;")
        .unwrap();
    assert!(TasksStore::open(&unversioned_view).is_err());
    let objects: Vec<(String, String)> = conn
        .prepare(
            "SELECT type,name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(objects, [("view".into(), "old_projection".into())]);

    let current = root.0.join("current.sqlite");
    drop(TasksStore::open(&current).expect("empty DB should initialize as schema v5"));
    let reopened = TasksStore::open(&current).expect("schema v5 should remain supported");
    let version: String = reopened
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "5");
}
#[test]
fn infer_project_from_path_supports_arbitrary_monorepo_structures_universally() {
    use contextunity_forge_mcp::core::tasks::infer_project_from_path;
    use std::path::Path;

    // Standard docs/milestones and docs/plans under arbitrary subprojects
    assert_eq!(
        infer_project_from_path(Path::new("extensions/commerce/docs/milestones/010.md")),
        Some("extensions.commerce".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/cli/docs/milestones/010.md")),
        Some("packages.cli".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("services/brain/docs/plans/010.md")),
        Some("services.brain".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("apps/web/docs/milestones/010.md")),
        Some("apps.web".into())
    );

    // Arbitrary custom folders - Forge does not dictate folder naming conventions
    assert_eq!(
        infer_project_from_path(Path::new("custom_dir/my-plugin/milestones/010.md")),
        Some("custom_dir.my-plugin".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new(
            "team_alpha/backend/analytics/docs/milestones/020.md"
        )),
        Some("team_alpha.backend.analytics".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("user_service/docs/milestones/010.md")),
        Some("user_service".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("foo.bar/docs/milestones/010.md")),
        infer_project_from_path(Path::new("foo/bar/docs/milestones/010.md"))
    );
    assert_ne!(
        infer_project_from_path(Path::new("a~d/docs/milestones/010.md")),
        infer_project_from_path(Path::new("a.b/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/my app/docs/milestones/010.md")),
        Some("packages.my~u32~app".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/über/docs/milestones/010.md")),
        Some("packages.~u252~ber".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("microservices/billing/docs/plans/030.md")),
        Some("microservices.billing".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("packages/api/docs/milestones/010.md")),
        infer_project_from_path(Path::new("services/api/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("standalone_tool/milestones/010.md")),
        Some("standalone_tool".into())
    );

    // Root repository milestones and plans have no subproject
    assert_eq!(
        infer_project_from_path(Path::new("docs/milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("docs/plans/010.md")),
        None
    );
    assert_eq!(infer_project_from_path(Path::new("plans/010.md")), None);

    let root = ScopedWorkspace::new("forge_project_hierarchy");
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\nmilestones:\n  - docs/milestones\n  - packages/api/docs/milestones\n  - services/api/docs/milestones\n  - plugins/api/docs/milestones\n");
    let task_block = "# Tasks\n```yaml\ntask_ref: shared\ntarget: Resolve one project\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    for (dir, project) in [
        ("packages/api", ""),
        ("services/api", ""),
        ("plugins/api", "project: explicit-api\n"),
    ] {
        root.write(
            &format!("{dir}/docs/milestones/010-shared.md"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    for (filename, project) in [
        ("010-default.md", ""),
        ("020-alpha.md", "project: alpha\n"),
        ("030-beta.md", "project: beta\n"),
    ] {
        root.write(
            &format!("docs/milestones/{filename}"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    let mut ids = std::collections::BTreeSet::new();
    for reference in [
        "packages/api/docs/milestones/010-shared.md",
        "services/api/docs/milestones/010-shared.md",
        "plugins/api/docs/milestones/010-shared.md",
        "docs/milestones/010-default.md",
        "docs/milestones/020-alpha.md",
        "docs/milestones/030-beta.md",
    ] {
        let response = tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(reference.to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        ids.insert(response["tasks"][0]["task_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(ids.len(), 6);
    assert!(ids.iter().any(|id| id.starts_with("explicit-api/")));
    assert!(ids.iter().any(|id| id.starts_with("forge-mcp/")));
    assert!(ids.iter().any(|id| id.starts_with("alpha/")));
    assert!(ids.iter().any(|id| id.starts_with("beta/")));
}

#[test]
fn completed_task_reopen_and_mcp_manage_action_lifecycle() {
    let spec = "---\nid: m-reopen\ntitle: Reopen Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: reopenable\ntarget: Deliver reopenable feature\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let root = ScopedWorkspace::new("forge_reopen");
    root.write("src/lib.rs", "pub fn reopenable() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-reopen.md", spec);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-reopen.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/m-reopen:reopenable";

    // Advance task through all 4 gates to completed
    let stages = ["contract", "build", "review", "deliver"];
    for (index, stage) in stages.iter().enumerate() {
        let worker = if index >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.into(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();

        let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
        let mut ev = evidence(&stored);
        if *stage == "deliver" {
            ev.proof = json!({
                "delivery_proof": {"status": "passed"}
            });
        }
        tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": serde_json::to_value(&ev).unwrap(),
            }))
            .unwrap(),
        )
        .unwrap();
    }

    // Verify task is completed in SQLite
    let completed_task = store.inspect(task_id).unwrap();
    assert_eq!(completed_task.status, "completed");
    assert!(completed_task.completed_at.is_some());
    assert!(completed_task.receipt.is_some());

    let milestone_path = root.0.join("docs/010-reopen.md");
    let completed_text = std::fs::read_to_string(&milestone_path).unwrap();

    // Subtask modifications fail with informative TASK_TERMINAL message
    let add_err = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap_err();
    assert!(add_err.to_string().contains("TASK_TERMINAL"));
    assert!(add_err.to_string().contains("reopen or reset"));

    // Reopen requires its durable milestone receipt to be available before SQLite changes.
    let held_path = root.0.join("docs/010-reopen-held.md");
    std::fs::rename(&milestone_path, &held_path).unwrap();
    assert!(tasks::reopen(&root.0, task_id).is_err());
    assert_eq!(store.inspect(task_id).unwrap().status, "completed");
    std::fs::rename(&held_path, &milestone_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        completed_text
    );

    // MCP task_manage with action: "reopen"
    let reopen_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reopen,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reopen_res["status"], "ready");
    assert_eq!(reopen_res["stage"], "contract");
    assert!(reopen_res["receipt"].is_null());
    assert!(reopen_res["completed_at"].is_null());
    let reopened_text = std::fs::read_to_string(&milestone_path).unwrap();
    assert!(!reopened_text.contains("status: completed"));
    assert!(!reopened_text.contains("receipt:"));

    // Reopened task can accept subtasks
    let sub = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap();
    assert_eq!(sub.subtask_ref, "sub-audit");
    assert_eq!(sub.status, "ready");

    // Reopened task can be claimed again at contract
    let re_claimed = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "auditor".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(re_claimed["status"], "in_progress");

    // MCP task_manage with action: "reset" resets in-progress claim
    let reset_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reset,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reset_res["status"], "ready");
}

#[test]
fn task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path() {
    let root = ScopedWorkspace::new("forge_milestone_ref_resolution");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );
    let milestone_content = "---\nid: m-tool-performance-and-storage-compaction\ntitle: Performance\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: compaction\ntarget: Compact storage\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/030-tool-performance-and-storage-compaction.md",
        milestone_content,
    );

    // 1. Sync by milestone ID ("m-tool-performance-and-storage-compaction")
    let sync_by_id = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_id["tasks"].as_array().unwrap().len(), 1);

    // List by milestone ID
    let list_by_id = tasks::list(
        &root.0,
        tasks::List {
            repository: Some("test".into()),
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_id["tasks"].as_array().unwrap().len(), 1);

    // 2. Sync by numeric prefix ("030" and "30")
    let sync_by_prefix = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_prefix["tasks"].as_array().unwrap().len(), 1);

    let list_by_prefix = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: Some("30".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_prefix["tasks"].as_array().unwrap().len(), 1);

    // 3. Sync by filename stem ("030-tool-performance-and-storage-compaction")
    let sync_by_stem = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030-tool-performance-and-storage-compaction".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_stem["tasks"].as_array().unwrap().len(), 1);

    // 4. Sync by full relative file path
    let sync_by_path = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(
                "docs/milestones/030-tool-performance-and-storage-compaction.md".into(),
            ),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_path["tasks"].as_array().unwrap().len(), 1);

    // 5. Sync with a workspace selector imports all workspace manifests
    let sync_all = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_all["tasks"].as_array().unwrap().len(), 1);

    // 6. Non-existent milestone reference produces descriptive error instead of raw os error 2
    let err = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-non-existent".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not found in workspace"),
        "error must be descriptive: {err}"
    );
}

#[test]
fn task_list_filters_milestone_status_and_controls_subtask_details() {
    let root = ScopedWorkspace::new("forge_task_list_archive_and_evidence");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let active_spec = "---\nid: m-active\ntitle: Active Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: active-task\ntarget: Active task target\nproof_policy: seam-test-first\nscope: [src/]\nsubtasks:\n  - subtask_ref: sub-1\n    title: Active subtask\n    status: in_progress\n    evidence: Secret heavy evidence blob\n```\n";
    root.write("docs/milestones/010-active.md", active_spec);

    let archived_spec = "---\nid: m-archived\ntitle: Archived Milestone\ndoc_type: contract\nstatus: completed\n---\n# Tasks\n```yaml\ntask_ref: archived-task\ntarget: Archived task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/archive/009-archived.md", archived_spec);

    let planned_spec = "---\nid: m-planned\ntitle: Planned Milestone\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: planned-task\ntarget: Planned task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/008-planned.md", planned_spec);

    let cancelled_active_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/archive/007-cancelled.md",
        cancelled_active_spec,
    );
    let cancelled_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: cancelled\nclosure:\n  reason: Replaced by a later milestone\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    for (spec, path) in [
        (active_spec, "docs/milestones/010-active.md"),
        (archived_spec, "docs/milestones/archive/009-archived.md"),
        (planned_spec, "docs/milestones/008-planned.md"),
        (
            cancelled_active_spec,
            "docs/milestones/archive/007-cancelled.md",
        ),
    ] {
        store
            .sync(&Milestone::parse(spec, "forge-mcp").unwrap(), path, &root.0)
            .unwrap();
    }
    // Simulate the pre-sync window: the manifest is cancelled, but its stored task has not been pruned.
    root.write("docs/milestones/archive/007-cancelled.md", cancelled_spec);

    // Active milestones are the default, regardless of task status selection.
    let active_only = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let tasks_arr = active_only["tasks"].as_array().unwrap();
    assert_eq!(tasks_arr.len(), 1);
    assert_eq!(tasks_arr[0]["task_id"], "forge-mcp/m-active:active-task");

    // Compact listings retain the subtask reference and status, omitting details.
    let subtasks = tasks_arr[0]["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 1);
    assert_eq!(subtasks[0]["subtask_ref"], "sub-1");
    assert!(subtasks[0].get("title").is_none());
    assert_eq!(subtasks[0]["status"], "in_progress");
    assert!(
        subtasks[0].get("evidence").is_none(),
        "subtask evidence must be stripped in task_list"
    );

    let full_detail = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            detail: Some(tasks::TaskListDetail::Full),
            ..Default::default()
        },
    )
    .unwrap();
    let full_subtask = &full_detail["tasks"][0]["subtasks"][0];
    assert_eq!(full_subtask["title"], "Active subtask");
    assert_eq!(full_subtask["evidence"], "Secret heavy evidence blob");

    // The only explicit milestone filters are active, planned, completed, and all.
    for (filter, expected_id) in [
        (
            tasks::MilestoneStatusFilter::Active,
            "forge-mcp/m-active:active-task",
        ),
        (
            tasks::MilestoneStatusFilter::Planned,
            "forge-mcp/m-planned:planned-task",
        ),
        (
            tasks::MilestoneStatusFilter::Completed,
            "forge-mcp/m-archived:archived-task",
        ),
    ] {
        let filtered = tasks::list(
            &root.0,
            tasks::List {
                milestone_status: Some(filter),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["tasks"][0]["task_id"], expected_id);
    }

    let all_statuses = tasks::list(
        &root.0,
        tasks::List {
            milestone_status: Some(tasks::MilestoneStatusFilter::All),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let all_ids: Vec<_> = all_statuses["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(all_ids.len(), 4);
    assert!(all_ids.contains(&"forge-mcp/m-cancelled:cancelled-task"));

    // A targeted milestone reference exposes its tasks by default, including a pre-sync cancellation.
    let direct_archived = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("009".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let direct_arr = direct_archived["tasks"].as_array().unwrap();
    assert_eq!(direct_arr.len(), 1);
    assert_eq!(
        direct_arr[0]["task_id"],
        "forge-mcp/m-archived:archived-task"
    );
    let direct_cancelled = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("007".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(direct_cancelled["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        direct_cancelled["tasks"][0]["task_id"],
        "forge-mcp/m-cancelled:cancelled-task"
    );
}

#[test]
fn scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff() {
    let root = ScopedWorkspace::new("forge_snapshot_test");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";
    root.write("docs/milestones/010-test.md", spec);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    // Initialize git repository without tracking runtime state.
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Tester"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "src", "docs", "forge-mcp.yaml"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root.0)
        .status()
        .unwrap();

    let tasks_in_store = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id = &tasks_in_store[0].task_id;
    let target = tasks_in_store[0].spec.target.clone();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("release-notes".into()),
            title: Some("Publish release notes\nand notify maintainers".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("cleanup-followup".into()),
            title: Some("Optional cleanup follow-up".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("release-notes".into()),
            subtask_status: Some("completed".into()),
            evidence: Some("Release notes reviewed".into()),
            ..Default::default()
        },
    )
    .unwrap();
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");

    // 1. Claim contract
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Submit contract with commit: None -> automatically captures snapshot
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let snapshot_refs = root.0.join(".git/refs/forge/snapshots");
    std::fs::create_dir_all(&snapshot_refs).unwrap();
    let ref_blocker = snapshot_refs.join("forge-mcp");
    std::fs::write(&ref_blocker, "block the nested snapshot ref path").unwrap();
    let failed_pin = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        failed_pin.contains("TASK_SNAPSHOT_REF_FAILED"),
        "{failed_pin}"
    );
    let still_claimed = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(still_claimed.status, "in_progress");
    assert_eq!(still_claimed.gate, 0);
    std::fs::remove_file(ref_blocker).unwrap();

    let contract_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let contract_snap = pinned_candidate(&root.0, task_id);
    assert_eq!(contract_snap.len(), 40);
    assert_eq!(contract_res["snapshot"]["commit"], contract_snap);
    assert_eq!(
        contract_res["snapshot"]["inspect_cmd"],
        format!("git show {}", &contract_snap[..7])
    );

    // Commit a sibling change outside the task scope while keeping scoped content fixed.
    root.write("README.md", "# sibling change\n");
    std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "sibling README change"])
        .current_dir(&root.0)
        .status()
        .unwrap();

    // Claim build
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "build".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Submit build with commit: None -> captures snapshot with the untracked file!
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    ev.commit = Some("caller-selected-commit".into());
    let scoped_snapshot_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        scoped_snapshot_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != scoped snapshot {contract_snap}. Do: resubmit with commit {contract_snap}."
        )
    );
    ev.commit = None;
    let build_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let build_snap = pinned_candidate(&root.0, task_id);
    assert_eq!(build_snap.len(), 40);
    assert_eq!(build_res["snapshot"]["commit"], build_snap);
    assert_eq!(
        build_snap, contract_snap,
        "an unrelated sibling commit must not change the scoped candidate SHA"
    );

    // Verify git show on build snapshot includes the newly added file
    let show_output = std::process::Command::new("git")
        .args(["show", &build_snap])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let show_text = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_text.contains("new_feature.rs"));
    assert!(show_text.contains("pub fn new_feature() -> i32 { 42 }"));
    assert!(
        !show_text.contains("README.md"),
        "snapshot must contain only scoped files"
    );
    let parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", &build_snap])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(parents.status.success());
    assert_eq!(
        String::from_utf8_lossy(&parents.stdout)
            .split_whitespace()
            .count(),
        1,
        "root snapshots must not include a branch parent"
    );

    // 3. Independent review: the gate rejects reusing the builder's worker id.
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "review".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut review_store = tasks::store(&root.0).unwrap();
    let same_worker = review_store.inspect(task_id).unwrap();
    let same_worker_evidence = evidence(&same_worker);
    assert_eq!(
        review_store
            .submit(task_id, "review", &same_worker_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_REVIEW_NOT_INDEPENDENT: task '{task_id}'. reviewer worker 'builder' matches builder 'builder'. Do: submit review with a worker_id other than 'builder'."
        )
    );
    review_store
        .release_failed_claim(task_id, same_worker.claim_revision)
        .unwrap();
    drop(review_store);
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "review".into(),
            worker_id: "reviewer".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut review_store = tasks::store(&root.0).unwrap();
    let review_task = review_store.inspect(task_id).unwrap();
    let mut wrong_review_commit = evidence(&review_task);
    wrong_review_commit.commit = Some("caller-selected-commit".into());
    assert_eq!(
        review_store
            .submit(task_id, "review", &wrong_review_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );
    let explicit_review_commit_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&wrong_review_commit).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        explicit_review_commit_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );

    // Verify: If worktree diverges after build, review fails with TASK_CANDIDATE_MISMATCH!
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 99 }\n");
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let mismatch_err = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        mismatch_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let review_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    // Review snapshot matches build candidate snapshot
    assert_eq!(review_res["snapshot"]["commit"], build_snap);

    // 4. Delivery
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: "delivery".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut delivery_store = tasks::store(&root.0).unwrap();
    let delivery_task = delivery_store.inspect(task_id).unwrap();
    let missing_delivery_commit = evidence(&delivery_task);
    let (candidate_snapshot, _) = delivery_store
        .build_snapshot_candidate(task_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        delivery_store
            .submit(task_id, "deliver", &missing_delivery_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{task_id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."
        )
    );

    delivery_store
        .connection
        .execute(
            "UPDATE task_gates SET evidence=json_set(evidence, '$.commit', ?1) \
             WHERE task_id=?2 AND gate='review' AND state='passed'",
            rusqlite::params!["tampered-review-commit", task_id],
        )
        .unwrap();
    let review_source_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&missing_delivery_commit).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        review_source_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. review source 'review' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}."
        )
    );
    delivery_store
        .connection
        .execute(
            "UPDATE task_gates SET evidence=json_set(evidence, '$.commit', ?1) \
             WHERE task_id=?2 AND gate='review' AND state='passed'",
            rusqlite::params![candidate_snapshot, task_id],
        )
        .unwrap();
    drop(delivery_store);

    // Verify: If worktree diverges after review, delivery fails!
    root.write(
        "src/new_feature.rs",
        "pub fn new_feature() -> i32 { 999 }\n",
    );
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let deliver_err = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        deliver_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let delivery_baseline = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(delivery_baseline.status.success());
    let delivery_baseline = String::from_utf8_lossy(&delivery_baseline.stdout)
        .trim()
        .to_owned();
    let milestone_path = root.0.join("docs/milestones/010-test.md");
    let milestone_before_delivery = std::fs::read_to_string(&milestone_path).unwrap();

    // Keep an unrelated staged edit in the real index while delivery uses its
    // isolated candidate tree. Delivery must preserve both the staged entry
    // and the committed baseline README in its parented tree.
    root.write("README.md", "# unrelated staged edit\n");
    assert!(std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let unrelated_staged_before = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z", "--", "README.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(unrelated_staged_before.status.success());
    let unrelated_staged_before = unrelated_staged_before.stdout;
    let index_before_delivery = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(index_before_delivery.status.success());
    let index_before_delivery = index_before_delivery.stdout;

    // A rejected hook must leave both the branch and task receipt untouched.
    let hooks = root.0.join(".git/forced-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 47\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let hook_path = hooks.to_string_lossy().to_string();
    assert!(std::process::Command::new("git")
        .args(["config", "core.hooksPath", &hook_path])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let hook_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        hook_error.contains("TASK_DELIVERY_HOOK_REJECTED"),
        "{hook_error}"
    );
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let hook_head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&hook_head.stdout).trim(),
        delivery_baseline
    );
    let hook_index = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(hook_index.stdout, index_before_delivery);
    let after_hook_failure = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_hook_failure.status, "in_progress");
    assert_eq!(after_hook_failure.stage, "deliver");
    assert!(std::process::Command::new("git")
        .args(["config", "--unset", "core.hooksPath"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());

    // A SQLite statement failure after the hooked commit must roll back the
    // candidate branch and restore the original milestone file as well.
    let db_path = root.0.join(".forge/tasks.sqlite");
    let trigger_db = rusqlite::Connection::open(&db_path).unwrap();
    trigger_db
        .execute_batch(
            "CREATE TRIGGER reject_delivery_gate BEFORE INSERT ON task_gates
             WHEN NEW.gate='deliver' BEGIN SELECT RAISE(ABORT,'forced delivery sqlite failure'); END;",
        )
        .unwrap();
    drop(trigger_db);
    let sqlite_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        sqlite_error.contains("forced delivery sqlite failure"),
        "{sqlite_error}"
    );
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .execute_batch("DROP TRIGGER reject_delivery_gate;")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let sqlite_rollback_head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&sqlite_rollback_head.stdout).trim(),
        delivery_baseline
    );
    let sqlite_rollback_index = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(sqlite_rollback_index.stdout, index_before_delivery);
    let after_sqlite_failure = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_sqlite_failure.status, "in_progress");
    assert_eq!(after_sqlite_failure.stage, "deliver");

    let deliver_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let delivered_task = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let delivery_commit = delivered_task
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .unwrap()
        .to_owned();
    ev.commit = Some("caller-selected-commit".into());
    assert_eq!(
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "deliver".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(&ev).unwrap(),
                findings: None,
            },
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != accepted receipt commit {delivery_commit}. Do: resubmit with commit {delivery_commit}."
        )
    );

    assert_eq!(deliver_res["status"], "completed");
    assert_eq!(
        deliver_res["snapshot"]["commit"], deliver_res["receipt"]["commit"],
        "delivery returns the auto-created commit SHA"
    );
    assert_eq!(delivery_commit.len(), 40);
    assert_ne!(delivery_commit, build_snap);
    assert_eq!(deliver_res["snapshot"]["commit"], delivery_commit);
    assert_eq!(deliver_res["receipt"]["commit"], delivery_commit);
    assert_eq!(
        deliver_res["receipt"]["commit"], delivery_commit,
        "the returned receipt must carry the delivery commit"
    );
    let delivery_message = std::process::Command::new("git")
        .args(["log", "-1", "--format=%B"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(delivery_message.status.success());
    let delivery_message = String::from_utf8(delivery_message.stdout).unwrap();
    let target_present = delivery_message.contains(&target);
    let completed_subtask_present =
        delivery_message.contains("Publish release notes and notify maintainers");
    let pending_subtask_excluded = !delivery_message.contains("Optional cleanup follow-up");
    let subtask_title_is_single_line =
        completed_subtask_present && !delivery_message.contains("\nand notify maintainers");
    assert!(
        target_present
            && completed_subtask_present
            && pending_subtask_excluded
            && subtask_title_is_single_line,
        "delivery commit must include task target/completed subtasks, exclude pending subtasks, and normalize title newlines; target_present={target_present}, completed_subtask_present={completed_subtask_present}, pending_subtask_excluded={pending_subtask_excluded}, subtask_title_is_single_line={subtask_title_is_single_line}, message={delivery_message:?}"
    );
    let unrelated_staged_after = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z", "--", "README.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(unrelated_staged_after.stdout, unrelated_staged_before);
    let clean_task_paths = std::process::Command::new("git")
        .args([
            "status",
            "--short",
            "--untracked-files=no",
            "--",
            "src/new_feature.rs",
            "docs/milestones/010-test.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(clean_task_paths.status.success());
    assert!(
        clean_task_paths.stdout.is_empty(),
        "delivery task paths must be clean in the real index and worktree: {}",
        String::from_utf8_lossy(&clean_task_paths.stdout)
    );

    let committed_milestone = std::process::Command::new("git")
        .args([
            "show",
            &format!("{delivery_commit}:docs/milestones/010-test.md"),
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed_milestone.status.success());
    let committed_text = String::from_utf8(committed_milestone.stdout).unwrap();
    let committed_spec = Milestone::parse(&committed_text, "forge-mcp").unwrap();
    assert_eq!(committed_spec.tasks[0].status.as_deref(), Some("completed"));
    assert_eq!(
        committed_spec.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "delivery commit must not embed its own SHA in the milestone receipt"
    );
    let committed_readme = std::process::Command::new("git")
        .args(["show", &format!("{delivery_commit}:README.md")])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed_readme.status.success());
    assert_eq!(committed_readme.stdout, b"# sibling change\n");
    assert!(std::process::Command::new("git")
        .args(["checkout", "HEAD", "--", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());

    let written = std::fs::read_to_string(root.0.join("docs/milestones/010-test.md")).unwrap();
    let milestone = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "the active milestone receipt stays commitless until handoff"
    );

    // Inspect details shows gates with inspect_cmd and latest_snapshot
    let details = tasks::store(&root.0)
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    assert_eq!(details["receipt"]["commit"], delivery_commit);
    assert_eq!(details["latest_snapshot"]["commit"], build_snap);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {}", &build_snap[..7])
    );
    for gate in details["gates"].as_array().unwrap() {
        let evidence: serde_json::Value =
            serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        let expected = if gate["stage"] == "deliver" {
            delivery_commit.as_str()
        } else {
            build_snap.as_str()
        };
        assert_eq!(gate["commit"], expected);
        assert_eq!(evidence["commit"], expected);
        if gate["stage"] == "build" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
        if gate["stage"] == "deliver" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
    }
    let sqlite_store = tasks::store(&root.0).unwrap();
    let build_evidence_json: String = sqlite_store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='build' AND state='passed'",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let build_evidence: serde_json::Value = serde_json::from_str(&build_evidence_json).unwrap();
    assert_eq!(build_evidence["commit"], build_snap);
    assert_eq!(build_evidence["candidate_baseline_head"], delivery_baseline);
    let delivery_evidence_json: String = sqlite_store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='deliver' AND state='passed'",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let delivery_evidence: serde_json::Value =
        serde_json::from_str(&delivery_evidence_json).unwrap();
    assert_eq!(delivery_evidence["commit"], delivery_commit);
    assert_eq!(
        delivery_evidence["candidate_baseline_head"],
        delivery_baseline
    );

    // Verify snapshot refs exist in git before handoff
    let refs_out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let refs_text = String::from_utf8_lossy(&refs_out.stdout);
    assert!(refs_text.contains("refs/forge/snapshots/"));

    // Complete task second (which transitioned from blocked to ready upon completion of first)
    let ready_tasks = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id_2 = ready_tasks[0].task_id.clone();
    let milestone_branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let milestone_branch = String::from_utf8_lossy(&milestone_branch.stdout)
        .trim()
        .to_owned();
    assert!(std::process::Command::new("git")
        .args(["checkout", "-b", "task-second-delivery"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    for (stage, worker) in [
        ("contract", "b1"),
        ("build", "b1"),
        ("review", "r1"),
        ("deliver", "d1"),
    ] {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id_2.clone(),
                stage: (*stage).into(),
                worker_id: (*worker).into(),
                worktree: root.0.to_str().unwrap().into(),
                ..Default::default()
            },
        )
        .unwrap();
        let stored = tasks::store(&root.0).unwrap().inspect(&task_id_2).unwrap();
        let mut proof = evidence(&stored);
        proof.commit = None;
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id_2.clone(),
                stage: (*stage).into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(proof).unwrap(),
                findings: None,
            },
        )
        .unwrap();
    }

    let second_delivery_commit = tasks::store(&root.0)
        .unwrap()
        .inspect(&task_id_2)
        .unwrap()
        .receipt
        .and_then(|receipt| receipt.commit)
        .unwrap();
    assert!(std::process::Command::new("git")
        .args(["checkout", &milestone_branch])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "task-second-delivery",
            "-m",
            "merge second task"
        ])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let merge_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let merge_commit = String::from_utf8_lossy(&merge_commit.stdout)
        .trim()
        .to_owned();
    for delivered in [&delivery_commit, &second_delivery_commit] {
        assert!(std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", delivered, &merge_commit])
            .current_dir(&root.0)
            .status()
            .unwrap()
            .success());
    }
    let merge_parent = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^1"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&merge_parent.stdout).trim(),
        delivery_commit,
        "the merge landing's first parent is the fast-forwarded first delivery"
    );

    // Now handoff milestone
    handoff_with_fixture_evidence(&root.0, "m-test", None, "cargo test", 2, 0).unwrap();

    let archive_path = root.0.join("docs/milestones/archive/010-test.md");
    let archived_text = std::fs::read_to_string(&archive_path).unwrap();
    let archived = Milestone::parse(&archived_text, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(delivery_commit.as_str()),
        "first task keeps its reachable accepted delivery pointer"
    );
    assert_eq!(
        archived.tasks[1]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(second_delivery_commit.as_str()),
        "second task keeps its reachable accepted delivery pointer across merge"
    );
    assert_ne!(delivery_commit, merge_commit);
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .receipt
            .unwrap()
            .commit
            .as_deref(),
        Some(delivery_commit.as_str())
    );
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(&task_id_2)
            .unwrap()
            .receipt
            .unwrap()
            .commit
            .as_deref(),
        Some(second_delivery_commit.as_str())
    );

    // Verify snapshot refs were pruned after milestone handoff!
    let refs_after = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let refs_after_text = String::from_utf8_lossy(&refs_after.stdout)
        .trim()
        .to_string();
    assert!(
        refs_after_text.is_empty(),
        "Expected all snapshot refs to be pruned on handoff, got: {refs_after_text}"
    );
}

#[test]
fn scoped_snapshot_omits_deleted_exact_path_during_contract_submit() {
    let root = ScopedWorkspace::new("forge_snapshot_deleted_exact_path");
    root.write("src/deleted.rs", "pub fn removed() {}\n");
    root.write("src/kept.rs", "pub fn kept() -> i32 { 1 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 2 }\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-snapshot\ntitle: Snapshot Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Snapshot Tasks\n```yaml\ntask_ref: exact-path\ntarget: Capture a scoped candidate with a deleted exact path\nproof_policy: seam-test-first\nscope: [src/deleted.rs, src/kept.rs]\n```\n";
    root.write("docs/milestones/010-snapshot.md", spec);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-snapshot.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap()
    };
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Tester"],
        vec!["add", "-A"],
        vec!["commit", "-q", "-m", "initial"],
    ] {
        let output = run_git(&args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let ready = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id = ready[0].task_id.clone();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    std::fs::remove_file(root.0.join("src/deleted.rs")).unwrap();
    root.write("src/kept.rs", "pub fn kept() -> i32 { 42 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 99 }\n");

    let stored = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
    let mut proof = evidence(&stored);
    proof.commit = None;
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.clone(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let snapshot = pinned_candidate(&root.0, &task_id);
    let task_tree = run_git(&["ls-tree", "-r", "--name-only", &snapshot]);
    assert!(task_tree.status.success());
    assert_eq!(
        String::from_utf8_lossy(&task_tree.stdout).trim(),
        "src/kept.rs"
    );
    let kept = run_git(&["show", &format!("{snapshot}:src/kept.rs")]);
    assert!(kept.status.success());
    assert_eq!(
        String::from_utf8_lossy(&kept.stdout),
        "pub fn kept() -> i32 { 42 }\n"
    );
    let submitted = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
    assert_eq!(submitted.gate, 1);
    assert_eq!(submitted.status, "ready");
}

#[test]
fn scope_extension_rejects_paths_owned_by_sibling_tasks_and_admits_unowned_tests_and_src() {
    let root = ScopedWorkspace::new("forge_scope_conflict");
    root.write("src/module_a/foo.rs", "pub fn foo() {}\n");
    root.write("src/module_b/bar.rs", "pub fn bar() {}\n");
    root.write("tests/test_foo.rs", "// test\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-conflict\ntitle: Conflict Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: task_a\ntarget: Deliver A\nproof_policy: seam-test-first\nscope: [src/module_a/foo.rs]\nscope_roots: [src/, tests/]\n```\n```yaml\ntask_ref: task_b\ntarget: Deliver B\nproof_policy: seam-test-first\nscope: [src/module_b/bar.rs]\nscope_roots: [src/, tests/]\n```\n";
    root.write("docs/010-conflict.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-conflict.md", &root.0)
        .unwrap();

    let task_b_id = "forge-mcp/m-conflict:task_b";
    let task_a_id = "forge-mcp/m-conflict:task_a";

    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_b_id.into(),
            stage: "contract".into(),
            worker_id: "worker_b".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let assert_scope_conflict = |requester: &str, path: &str, owner: &str, shape: &str| {
        let err = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "extend_scope",
                "task_id": requester,
                "paths": [path]
            }))
            .unwrap(),
        )
        .unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("TASK_SCOPE_CONFLICT"),
            "Expected TASK_SCOPE_CONFLICT for {shape}, got: {err_msg}"
        );
        assert!(
            err_msg.contains(owner),
            "Expected {shape} conflict to name owner '{owner}', got: {err_msg}"
        );
    };

    for (path, shape) in [
        ("src/module_a/foo.rs", "exact frozen path"),
        ("src/module_a/foo.rs/child.rs", "child of frozen path"),
        ("src/module_a", "parent of frozen path"),
    ] {
        assert_scope_conflict(task_b_id, path, task_a_id, shape);
    }

    // Unowned tests/src paths are admitted with frozen=0.
    for path in ["tests/test_foo.rs", "src/module_b/extra.rs"] {
        let admitted = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "extend_scope",
                "task_id": task_b_id,
                "paths": [path]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(admitted["status"], "in_progress");
    }

    // Dynamic extensions stay owned; an inactive requester must still fail closed.
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_a_id.into(),
            stage: "contract".into(),
            worker_id: "worker_a".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    store.reset(task_a_id).unwrap();

    let dynamic_count: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1 AND frozen=0",
            [task_b_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(dynamic_count, 2, "admitted tests/src paths stay unfrozen");

    for (path, shape) in [
        ("src/module_b/extra.rs", "exact unfrozen path"),
        ("src/module_b/extra.rs/child.rs", "child of unfrozen path"),
        ("src/module_b", "parent of unfrozen path"),
        ("tests/test_foo.rs/child.rs", "child of unfrozen test path"),
        ("tests", "parent of unfrozen test path"),
    ] {
        assert_scope_conflict(task_a_id, path, task_b_id, shape);
    }

    let claim_root = ScopedWorkspace::new("forge_claim_scope_conflict");
    claim_root.write("src/shared/owned.rs", "pub fn owned() {}\n");
    claim_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let claim_spec = "---\nid: m-claim-conflict\ntitle: Claim Conflict Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: owner\ntarget: Own shared path\nproof_policy: seam-test-first\nscope: [src/shared/owned.rs]\nscope_roots: [src/]\n```\n```yaml\ntask_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\nscope: [src/shared/owned.rs]\nscope_roots: [src/]\n```\n";
    let claim_spec = claim_spec.replace(
        "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\nscope:",
        "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ndepends_on: [owner]\nscope:",
    );
    claim_root.write("docs/010-claim-conflict.md", &claim_spec);
    let claim_milestone = Milestone::parse(&claim_spec, "forge-mcp").unwrap();
    let mut claim_store = TasksStore::open(&claim_root.0.join(".forge/tasks.sqlite")).unwrap();
    claim_store
        .sync(
            &claim_milestone,
            "docs/010-claim-conflict.md",
            &claim_root.0,
        )
        .unwrap();
    let owner_id = "forge-mcp/m-claim-conflict:owner";
    tasks::claim(
        &claim_root.0,
        tasks::Claim {
            task_id: owner_id.into(),
            stage: "contract".into(),
            worker_id: "owner".into(),
            worktree: claim_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let claim_spec = claim_spec
        .replace(
            "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ndepends_on: [owner]\nscope:",
            "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ncontract_revision: 2\nscope:",
        );
    claim_root.write("docs/010-claim-conflict.md", &claim_spec);
    let claim_milestone = Milestone::parse(&claim_spec, "forge-mcp").unwrap();
    claim_store
        .sync(
            &claim_milestone,
            "docs/010-claim-conflict.md",
            &claim_root.0,
        )
        .unwrap();
    let claim_error = tasks::claim(
        &claim_root.0,
        tasks::Claim {
            task_id: "forge-mcp/m-claim-conflict:successor".into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            worktree: claim_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(claim_error.contains("TASK_NOT_READY"), "{claim_error}");
    assert!(claim_error.contains(owner_id), "{claim_error}");

    let release_root = ScopedWorkspace::new("forge_completed_scope_release");
    release_root.write("src/released/owned.rs", "pub fn owned() {}\n");
    release_root.write("src/released/extra.rs", "pub fn extra() {}\n");
    release_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let release_spec = "---\nid: m-scope-release\ntitle: Scope Release Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: completed_owner\ntarget: Finish shared scope owner\nproof_policy: seam-test-first\nscope: [src/released/]\nscope_roots: [src/]\n```\n```yaml\ntask_ref: successor\ntarget: Reuse released scope\nproof_policy: seam-test-first\nscope: [src/released/owned.rs]\nscope_roots: [src/]\n```\n";
    release_root.write("docs/010-scope-release.md", release_spec);
    let mut release_milestone = Milestone::parse(release_spec, "forge-mcp").unwrap();
    release_milestone.tasks[0].status = Some("completed".into());
    release_milestone.tasks[0].receipt = Some(Receipt {
        commit: None,
        contract_revision: 1,
        passed_at: "2026-10-10T00:00:00Z".into(),
        evidence: json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}}),
        review: passing_review_proof(),
        decision: "pass".into(),
        rollup: None,
    });
    let mut release_store = TasksStore::open(&release_root.0.join(".forge/tasks.sqlite")).unwrap();
    release_store
        .sync(
            &release_milestone,
            "docs/010-scope-release.md",
            &release_root.0,
        )
        .unwrap();
    let completed_owner_id = "forge-mcp/m-scope-release:completed_owner";
    let retained_scope_count: i64 = release_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [completed_owner_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_scope_count, 1);
    let successor_id = "forge-mcp/m-scope-release:successor";
    let claimed = tasks::claim(
        &release_root.0,
        tasks::Claim {
            task_id: successor_id.into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            worktree: release_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(claimed["status"], "in_progress");
    let extended = tasks::manage(
        &release_root.0,
        serde_json::from_value(json!({
            "action": "extend_scope",
            "task_id": successor_id,
            "paths": ["src/released/extra.rs"]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(extended["status"], "in_progress");

    let (protected_root, mut protected_store, protected_milestone) = fixture();
    let protected_id = protected_milestone.task_id(&protected_milestone.tasks[0]);
    let before_extension = protected_store.inspect(&protected_id).unwrap();
    let before_paths: i64 = protected_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [&protected_id],
            |row| row.get(0),
        )
        .unwrap();
    let extension_error = protected_store
        .extend_scope(
            &protected_id,
            &[".forge/acdd/profile.yaml".into()],
            &protected_root.0,
        )
        .unwrap_err();
    assert!(extension_error.to_string().contains("TASK_SCOPE_PROTECTED"));
    let after_extension = protected_store.inspect(&protected_id).unwrap();
    assert_eq!(after_extension.spec.scope, before_extension.spec.scope);
    assert_eq!(after_extension.status, before_extension.status);
    let after_paths: i64 = protected_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [&protected_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after_paths, before_paths,
        "rejected extension added scope rows"
    );

    for tracked in [false, true] {
        let root = ScopedWorkspace::new(if tracked {
            "forge_protected_tracked"
        } else {
            "forge_protected_untracked"
        });
        root.write("src/lib.rs", "pub fn protected() {}\n");
        root.write(".forge/acdd/profile.yaml", "\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let contract = "---\nid: m-protected\ntitle: Protected scope\ndoc_type: contract\nstatus: active\n---\n# Protected scope\n```yaml\ntask_ref: protected\ntarget: Reject dirty protected configuration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
        root.write("docs/010-protected.md", contract);
        let milestone = Milestone::parse(contract, "forge-mcp").unwrap();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store
            .sync(&milestone, "docs/010-protected.md", &root.0)
            .unwrap();
        if tracked {
            let added = std::process::Command::new("git")
                .args(["add", "--", ".forge/acdd/profile.yaml"])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(added.status.success());
            let committed = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=ACDD test",
                    "-c",
                    "user.email=acdd-test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-m",
                    "track protected profile",
                ])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                committed.status.success(),
                "{}",
                String::from_utf8_lossy(&committed.stderr)
            );
            root.write(".forge/acdd/profile.yaml", "# changed after commit\n");
        }

        let before = store.inspect(&task_id).unwrap();
        let claims_before: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_claims WHERE task_id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        let error = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.clone(),
                stage: "contract".into(),
                worker_id: "protected-scope-check".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("TASK_SCOPE_VIOLATION"),
            "tracked={tracked}: {error:#}"
        );
        let after = store.inspect(&task_id).unwrap();
        assert_eq!(after.status, before.status);
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.claim_revision, before.claim_revision);
        let claims_after: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_claims WHERE task_id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            claims_after, claims_before,
            "tracked={tracked} claim mutated DB"
        );
    }

    let admitted_root = ScopedWorkspace::new("forge_protected_initial_scope");
    admitted_root.write(".forge/acdd/profile.yaml", "\n");
    admitted_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let admitted_contract = "---\nid: m-protected-admitted\ntitle: Protected scope admitted\ndoc_type: contract\nstatus: active\n---\n# Protected scope admitted\n```yaml\ntask_ref: protected\ntarget: Admit configuration at planning time\nproof_policy: seam-test-first\nscope: [.forge/acdd/profile.yaml]\nscope_roots: [.forge/acdd/]\n```\n";
    admitted_root.write("docs/010-protected-admitted.md", admitted_contract);
    let admitted_milestone = Milestone::parse(admitted_contract, "forge-mcp").unwrap();
    let admitted_id = admitted_milestone.task_id(&admitted_milestone.tasks[0]);
    let mut admitted_store =
        TasksStore::open(&admitted_root.0.join(".forge/tasks.sqlite")).unwrap();
    admitted_store
        .sync(
            &admitted_milestone,
            "docs/010-protected-admitted.md",
            &admitted_root.0,
        )
        .unwrap();
    let admitted = tasks::claim(
        &admitted_root.0,
        tasks::Claim {
            task_id: admitted_id.clone(),
            stage: "contract".into(),
            worker_id: "admitted-protected-scope".into(),
            worktree: admitted_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(admitted["status"], "in_progress");
    assert_eq!(
        admitted_store.inspect(&admitted_id).unwrap().initial_scope,
        vec![".forge/acdd/profile.yaml"]
    );
}

#[test]
fn claim_returns_bundle_by_default_with_task_and_milestone_blackboard_messages() {
    let (root, store, _) = fixture();
    let task_id = "forge-mcp/m-test:first";

    // Post message to task
    store
        .blackboard_post(task_id, "agent_1", "notes", "Initial hypothesis for task")
        .unwrap();

    // Post message from another task in same milestone
    let task_2_id = "forge-mcp/m-test:second";
    store
        .blackboard_post(task_2_id, "agent_2", "decisions", "Milestone level finding")
        .unwrap();

    // Claim without specifying bundle (bundle defaults to true)
    let claim_res = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    // Response should be the full bundle by default
    assert_eq!(claim_res["task_id"], task_id);
    assert_eq!(claim_res["context_bundle"]["contract"]["task_id"], task_id);
    assert!(
        claim_res["context_bundle"].get("proof_schema").is_none(),
        "built-in proof gates must not expose a scheme proof schema"
    );
    let bb = claim_res["blackboard"]
        .as_array()
        .expect("blackboard array in bundle");
    assert_eq!(
        bb.len(),
        2,
        "Expected both task and milestone sibling blackboard messages"
    );
    assert!(claim_res["workflow_guidance"].is_object());
}

#[test]
fn task_blackboard_gate_targeting_and_schema_hint() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);

    let schema_version: String = store
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "5");
    let gate_column_exists: bool = store
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_blackboard') WHERE name='gate')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(gate_column_exists, "current schema stores gate targeting");
    let gate_indexes: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%gate%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(gate_indexes > 0, "gate-targeted lookups need an index");
    drop(store);

    let call = |request| {
        tasks::blackboard(
            &root.0,
            serde_json::from_value(request).unwrap(),
            "blackboard-test",
        )
    };
    let contract_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "decisions",
        "payload": "Choose the public gate routing shape"
    }))
    .unwrap();
    let contract_id = contract_post["id"].as_u64().unwrap();
    let build_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "build",
        "topic": "deferred",
        "payload": "Retain the build follow-up"
    }))
    .unwrap();
    let build_id = build_post["id"].as_u64().unwrap();

    let contract_page = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "contract",
        "limit": 50
    }))
    .unwrap();
    let contract_messages = contract_page["messages"].as_array().unwrap();
    assert_eq!(contract_messages.len(), 1);
    assert_eq!(contract_messages[0]["id"], contract_id);
    assert_eq!(contract_messages[0]["gate"], "contract");

    let invalid_topic = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "architectural_notes",
        "payload": "This legacy topic must be rejected at runtime"
    }))
    .unwrap_err();
    let schema_hint = format!("{invalid_topic:#}");
    for expected in [
        "draft",
        "notes",
        "findings",
        "blockers",
        "decisions",
        "deferred",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            schema_hint.contains(expected),
            "actionable blackboard schema hint omitted {expected:?}: {schema_hint}"
        );
    }
    let invalid_gate = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "unknown-gate"
    }))
    .unwrap_err();
    let gate_hint = format!("{invalid_gate:#}");
    for expected in [
        "unknown-gate",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            gate_hint.contains(expected),
            "actionable blackboard gate hint omitted {expected:?}: {gate_hint}"
        );
    }

    let claim = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.clone(),
            stage: "contract".into(),
            worker_id: "blackboard-claim-test".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let claim_messages = claim["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .expect("claim guidance carries the active gate's blackboard messages");
    assert_eq!(claim_messages.len(), 1);
    assert_eq!(claim_messages[0]["id"], contract_id);
    assert_eq!(claim_messages[0]["gate"], "contract");
    let blackboard_info = claim["blackboard_info"]
        .as_object()
        .expect("claim returns a compact blackboard_info summary");
    assert!(
        serde_json::to_vec(blackboard_info).unwrap().len() <= 1024,
        "blackboard_info should remain compact"
    );
    assert_ne!(contract_id, build_id);
}

#[test]
fn universal_scope_roots_extend_scope_and_boundary_validation() {
    let root = ScopedWorkspace::new("forge_universal_scope");
    root.write("packages/catalogue/models/product.py", "# product\n");
    root.write(
        "packages/catalogue/services/product_service.py",
        "# service\n",
    );
    root.write("tests/catalogue/test_product.py", "# test\n");
    root.write("docs/architecture/catalogue/overview.md", "# doc\n");
    root.write("packages/auth/models/user.py", "# auth\n");
    root.write("services/payment/handler.py", "# payment\n");
    root.write("services/payment/utils.py", "# payment utils\n");
    root.write("services/billing/invoice.py", "# billing\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [packages, services]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );

    let spec = "---\nid: m-universal\ntitle: Universal Scope Tasks\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: catalogue_task\ntarget: Build catalogue service\nproof_policy: seam-test-first\nscope: [packages/catalogue/models/product.py]\nscope_roots:\n  - packages/catalogue/\n  - tests/catalogue/\n  - docs/architecture/catalogue/\n```\n```yaml\ntask_ref: payment_task\ntarget: Build payment handler\nproof_policy: seam-test-first\nscope: [services/payment/handler.py]\n```\n";
    root.write("docs/010-universal.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-universal.md", &root.0)
        .unwrap();

    let catalogue_id = "forge-mcp/m-universal:catalogue_task";
    let payment_id = "forge-mcp/m-universal:payment_task";

    // 1. Extending within declared scope_roots succeeds
    let ext1 = store
        .extend_scope(
            catalogue_id,
            &[
                "packages/catalogue/services/product_service.py".into(),
                "tests/catalogue/test_product.py".into(),
                "docs/architecture/catalogue/overview.md".into(),
            ],
            &root.0,
        )
        .unwrap();
    assert!(ext1
        .spec
        .scope
        .contains(&"packages/catalogue/services/product_service.py".into()));
    assert!(ext1
        .spec
        .scope
        .contains(&"tests/catalogue/test_product.py".into()));
    assert!(ext1
        .spec
        .scope
        .contains(&"docs/architecture/catalogue/overview.md".into()));

    // 2. Extending outside declared scope_roots fails (even for arbitrary paths or src/)
    let err_auth = store
        .extend_scope(
            catalogue_id,
            &["packages/auth/models/user.py".into()],
            &root.0,
        )
        .unwrap_err();
    assert!(
        err_auth.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID, got: {err_auth}"
    );

    let err_src = store
        .extend_scope(catalogue_id, &["src/anything.rs".into()], &root.0)
        .unwrap_err();
    assert!(
        err_src.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for src outside scope_roots, got: {err_src}"
    );

    // 3. Fallback when scope_roots is omitted: auto-derives base from initial scope
    let ext_payment = store
        .extend_scope(payment_id, &["services/payment/utils.py".into()], &root.0)
        .unwrap();
    assert!(ext_payment
        .spec
        .scope
        .contains(&"services/payment/utils.py".into()));

    let err_billing = store
        .extend_scope(payment_id, &["services/billing/invoice.py".into()], &root.0)
        .unwrap_err();
    assert!(
        err_billing.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for outside auto-derived base, got: {err_billing}"
    );

    // 4. Root-level file without scope_roots does not open repository tree
    root.write("install.sh", "#!/bin/sh\n");
    root.write("Cargo.toml", "[package]\n");
    let root_spec = "---\nid: m-root\ntitle: Root File Task\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: installer_task\ntarget: Deliver installer\nproof_policy: direct-proof\nscope: [install.sh]\n```\n";
    root.write("docs/010-root.md", root_spec);
    let root_milestone = Milestone::parse(root_spec, "forge-mcp").unwrap();
    store
        .sync(&root_milestone, "docs/010-root.md", &root.0)
        .unwrap();
    let installer_id = "forge-mcp/m-root:installer_task";

    let err_root_cargo = store
        .extend_scope(installer_id, &["Cargo.toml".into()], &root.0)
        .unwrap_err();
    assert!(
        err_root_cargo.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for Cargo.toml from root-level install.sh, got: {err_root_cargo}"
    );

    let err_root_src = store
        .extend_scope(installer_id, &["src/lib.rs".into()], &root.0)
        .unwrap_err();
    assert!(
        err_root_src.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for src/lib.rs from root-level install.sh, got: {err_root_src}"
    );

    // 5. Empty scope_roots is omitted from serialization to preserve task digest compatibility
    let serialized_task = serde_json::to_string(&milestone.tasks[1]).unwrap();
    assert!(
        !serialized_task.contains("scope_roots"),
        "expected empty scope_roots to be omitted from serialized JSON, got: {serialized_task}"
    );
    let digest_without_field = milestone.digest(&milestone.tasks[1]).unwrap();
    let mut task_with_empty_vec = milestone.tasks[1].clone();
    task_with_empty_vec.scope_roots = Vec::new();
    let digest_with_empty_vec = milestone.digest(&task_with_empty_vec).unwrap();
    assert_eq!(
        digest_without_field, digest_with_empty_vec,
        "empty scope_roots must produce identical digest to omitted field"
    );

    // 6. Milestone::parse rejects tasks where initial scope does not fall into scope_roots
    let invalid_spec = "---\nid: m-inv\ntitle: Invalid\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: bad\ntarget: Bad\nproof_policy: direct-proof\nscope: [packages/auth/user.py]\nscope_roots: [packages/catalogue/]\n```\n";
    let parse_err = Milestone::parse(invalid_spec, "forge-mcp").unwrap_err();
    assert!(
        parse_err.to_string().contains("TASK_SCOPE_INVALID"),
        "expected parse error for scope outside scope_roots, got: {parse_err}"
    );
}

#[test]
fn declarative_profile_compiles_defaults_and_respects_role_overrides() {
    use contextunity_forge_mcp::core::tasks::profile::{
        compiled, load_for_workspace, parse_profile, RoleDef, RoleExecutionMode,
    };

    let role_defaults = RoleDef::default();
    assert_eq!(role_defaults.mode, None);
    assert_eq!(role_defaults.reuse_on_reject, None);
    assert_eq!(role_defaults.effective_mode(), RoleExecutionMode::Subagent);
    assert!(role_defaults.effective_reuse_on_reject());

    // 1. Embedded roles define the default execution and rejection policies without
    //    duplicating ModelSpec's model and reasoning fields at the role level.
    let default_profile = compiled();
    assert_eq!(default_profile.gates.len(), 4);
    assert_eq!(default_profile.gate_id(0), "contract");
    assert_eq!(default_profile.gate_id(1), "build");
    assert_eq!(default_profile.gate_id(2), "review");
    assert_eq!(default_profile.gate_id(3), "deliver");

    let reviewer_role = default_profile
        .roles
        .get("independent_reviewer")
        .expect("reviewer role exists");
    let reviewer_value = serde_json::to_value(reviewer_role).unwrap();
    assert_eq!(reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(reviewer_value["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(
        reviewer_value["models"],
        serde_json::json!([{"model":"sol-6.1","reasoning":"xhigh"}])
    );
    assert!(
        reviewer_value.get("model").is_none(),
        "model belongs to ModelSpec"
    );
    assert!(
        reviewer_value.get("reasoning").is_none(),
        "reasoning belongs to ModelSpec"
    );
    assert!(
        reviewer_role.guidance.is_some(),
        "default reviewer must provide textual guidance"
    );

    // 2. Strict deny_unknown_fields fails closed on unknown YAML keys
    let invalid_yaml = "unknown_field: true\ngates: []\n";
    let parse_err = parse_profile(invalid_yaml).unwrap_err();
    assert!(
        parse_err
            .to_string()
            .contains("ACDD_PROFILE_SCHEMA_INVALID"),
        "expected ACDD_PROFILE_SCHEMA_INVALID error on unknown field, got: {parse_err}"
    );

    // 3. Repository override replaces mode, rejection policy, and ordered models.
    let ws = crate::common::Workspace::new();
    ws.write(
        ".forge/acdd/profile.yaml",
        "roles:\n  independent_reviewer:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: sol-6.1\n        reasoning: high\n      - model: sol-6.0\n        reasoning: medium\n",
    );

    let loaded = load_for_workspace(ws.root()).unwrap();
    let loaded_reviewer = loaded.roles.get("independent_reviewer").unwrap();
    let loaded_reviewer_value = serde_json::to_value(loaded_reviewer).unwrap();
    assert_eq!(loaded_reviewer_value["mode"], serde_json::json!("inline"));
    assert_eq!(
        loaded_reviewer_value["reuse_on_reject"],
        serde_json::json!(false)
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["model"],
        serde_json::json!("sol-6.1")
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("high")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["model"],
        serde_json::json!("sol-6.0")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(loaded_reviewer_value.get("model").is_none());
    assert!(loaded_reviewer_value.get("reasoning").is_none());
    assert!(
        loaded_reviewer.guidance.is_some(),
        "the base guidance is inherited"
    );
    // Other gates and roles remain preserved from defaults
    assert_eq!(loaded.gates.len(), 4);
    assert!(loaded.roles.contains_key("builder"));

    // 4. Omitted role overrides inherit the base defaults and a model list replaces as a unit.
    ws.write(
        "custom/team_profile.yaml",
        "roles:\n  independent_reviewer:\n    models:\n      - model: custom-model\n        reasoning: medium\n",
    );
    ws.write("forge-mcp.yaml", "acdd_profile: custom/team_profile.yaml\n");
    let loaded_custom = load_for_workspace(ws.root()).unwrap();
    let custom_reviewer = loaded_custom.roles.get("independent_reviewer").unwrap();
    let custom_reviewer_value = serde_json::to_value(custom_reviewer).unwrap();
    assert_eq!(custom_reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(
        custom_reviewer_value["reuse_on_reject"],
        serde_json::json!(true)
    );
    assert_eq!(custom_reviewer_value["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        custom_reviewer_value["models"][0]["model"],
        serde_json::json!("custom-model")
    );
    assert_eq!(
        custom_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(custom_reviewer_value.get("model").is_none());
    assert!(custom_reviewer_value.get("reasoning").is_none());

    // Direct field-wise merge keeps base fields when an override omits them or
    // supplies an empty model list, then replaces the ordered models as a unit.
    let mut base = parse_profile(
        "roles:\n  builder:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: base-model\n        reasoning: high\n    guidance: base guidance\n",
    )
    .unwrap();
    base.merge_override(
        parse_profile(
            "roles:\n  builder:\n    mode: null\n    reuse_on_reject: null\n    models: []\n",
        )
        .unwrap(),
    );
    let inherited = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(inherited["mode"], serde_json::json!("inline"));
    assert_eq!(inherited["reuse_on_reject"], serde_json::json!(false));
    assert_eq!(
        inherited["models"][0]["model"],
        serde_json::json!("base-model")
    );
    assert_eq!(inherited["guidance"], serde_json::json!("base guidance"));
    base.merge_override(parse_profile(
        "roles:\n  builder:\n    mode: subagent\n    reuse_on_reject: true\n    models:\n      - model: replacement-model\n        reasoning: medium\n",
    )
    .unwrap());
    let replaced = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(replaced["mode"], serde_json::json!("subagent"));
    assert_eq!(replaced["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(replaced["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        replaced["models"][0]["model"],
        serde_json::json!("replacement-model")
    );
    assert_eq!(replaced["guidance"], serde_json::json!("base guidance"));

    // String model notation parses model:reasoning and bare model strings
    let string_models = parse_profile(
        "roles:\n  builder:\n    models:\n      - 'gpt6-luna:xhigh'\n      - 'gemini-3.8:high'\n      - 'composer-2.5'\n",
    )
    .unwrap();
    let builder_models = &string_models.roles.get("builder").unwrap().models;
    assert_eq!(builder_models.len(), 3);
    assert_eq!(builder_models[0].model, "gpt6-luna");
    assert_eq!(builder_models[0].reasoning.as_deref(), Some("xhigh"));
    assert_eq!(builder_models[0].formatted(), "gpt6-luna:xhigh");
    assert_eq!(builder_models[1].formatted(), "gemini-3.8:high");
    assert_eq!(builder_models[2].formatted(), "composer-2.5");

    // Invalid string models with empty segments fail closed
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - 'model:'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ':high'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ''\n").is_err());
    // Map models with unknown fields fail closed
    assert!(parse_profile(
        "roles:\n  builder:\n    models:\n      - model: foo\n        unknown_field: bar\n"
    )
    .is_err());

    // The documented reference profile example loads and validates cleanly
    let example_profile_text =
        std::fs::read_to_string("skills/contextunity-forge/references/acdd_profile.yaml.example")
            .expect("example profile file must exist");
    let parsed_example = parse_profile(&example_profile_text)
        .expect("example profile must parse and validate without error");
    assert!(!parsed_example.roles.is_empty());
    // exactly one terminal delivery gate instead of accepting an incomplete workflow.
    let invalid_workflow = crate::common::Workspace::new();
    invalid_workflow.write(
        ".forge/acdd/profile.yaml",
        "gates:\n  - id: inspect\n    role: builder\n    proof: none\n",
    );
    let invalid_profile = load_for_workspace(invalid_workflow.root());
    assert!(
        invalid_profile.is_err(),
        "a merged profile without exactly one terminal delivery gate must fail closed"
    );

    let valid_gates = r#"gates:
  - id: work
    role: builder
    proof: none
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: work
    independent_from: work
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: work
"#;
    let later_gate = "  - id: later\n    role: builder\n    proof: none\n";
    let assert_profile_error = |case: &str, yaml: &str, expected: &str| {
        let workspace = crate::common::Workspace::new();
        workspace.write(".forge/acdd/profile.yaml", yaml);
        let error = match load_for_workspace(workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains(expected),
            "{case} expected diagnostic {expected:?}, got: {detail}"
        );
    };
    let invalid_token = valid_gates
        .replacen("id: work", "id: work/v1", 1)
        .replacen("reject_to: work", "reject_to: work/v1", 2)
        .replacen("independent_from: work", "independent_from: work/v1", 1);
    assert_profile_error(
        "invalid gate token",
        &invalid_token,
        "gate id 'work/v1' must be a non-empty token",
    );
    let gates_without_receipt_review = valid_gates.replace("    receipt_review: review\n", "");
    let missing_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [absent]",
        1,
    );
    assert_profile_error(
        "missing review source",
        &missing_review_source,
        "review_sources must reference strictly prior review gates",
    );
    let non_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [work]",
        1,
    );
    assert_profile_error(
        "non-review review source",
        &non_review_source,
        "review source 'work' is not a review gate",
    );
    let cases = [
        (
            "duplicate gate id",
            valid_gates.replacen(
                "  - id: review\n    role: independent_reviewer\n    proof: review",
                "  - id: work\n    role: builder\n    proof: none\n  - id: review\n    role: independent_reviewer\n    proof: review",
                1,
            ),
        ),
        (
            "forward reject_to",
            valid_gates
                .replacen("reject_to: work", "reject_to: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing reject_to",
            valid_gates.replacen("reject_to: work", "reject_to: absent", 1),
        ),
        (
            "forward independent_from",
            valid_gates
                .replacen("independent_from: work", "independent_from: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing independent_from",
            valid_gates.replacen("independent_from: work", "independent_from: absent", 1),
        ),
        (
            "missing receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: absent", 1),
        ),
        (
            "non-review receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: work", 1),
        ),
        (
            "undefined role",
            valid_gates.replacen("role: builder", "role: missing_role", 1),
        ),
        (
            "undefined contour set",
            valid_gates.replacen("contours: standard", "contours: missing_contours", 1),
        ),
        (
            "empty command value",
            format!("commands:\n  blank: '  '\n{valid_gates}"),
        ),
        (
            "empty command registry name",
            format!("commands:\n  '': echo\n{valid_gates}"),
        ),
        (
            "array schema without items",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: array",
                1,
            ),
        ),
        (
            "schema required property is undefined",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        required: [missing]\n        properties:\n          nested:\n            type: string",
                1,
            ),
        ),
        (
            "invalid nested schema type",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        properties:\n          nested:\n            type: unsupported",
                1,
            ),
        ),
    ];
    for (case, yaml) in cases {
        let workspace = crate::common::Workspace::new();
        workspace.write(".forge/acdd/profile.yaml", &yaml);
        let error = match load_for_workspace(workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains("ACDD_PROFILE_INVALID")
                || detail.contains("ACDD_PROFILE_SCHEMA_INVALID"),
            "{case} failed for an unexpected reason: {detail}"
        );
    }

    let empty_registry = parse_profile(
        "roles:\n  builder: {}\n  independent_reviewer: {}\ncommands: {}\ngates:\n  - id: work\n    role: builder\n    proof: command\n  - id: review\n    role: independent_reviewer\n    proof: review\n  - id: deliver\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [review]\n",
    )
    .unwrap();
    let error = empty_registry.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires a non-empty commands registry"),
        "unexpected empty-registry validation error: {error:#}"
    );
}

#[test]
fn claim_context_exposes_recursive_scheme_schema_from_custom_profile() {
    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "pub fn schema_gate() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom/profile.yaml\n",
    );
    workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    workspace.write(
        "AGENTS.md",
        "# Task instructions\nLoad the contextunity-forge skill.\n",
    );
    workspace.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    workspace.write(
        "custom/profile.yaml",
        r#"
gates:
  - id: schema_input
    role: builder
    proof:
      scheme:
        type: object
        required: [report]
        additionalProperties: false
        properties:
          report:
            type: object
            required: [entries]
            additionalProperties: false
            properties:
              entries:
                type: array
                items:
                  type: object
                  required: [name]
                  additionalProperties: false
                  properties:
                    name:
                      type: string
                      enum: [accepted, rejected]
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: schema_input
    reject_to: schema_input
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: schema_input
    auto_commit: false
"#,
    );
    workspace.write(
        "docs/010-scheme.md",
        "---\nid: m-scheme\ntitle: Scheme proof\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Scheme proof\n```yaml\ntask_ref: schema\ntarget: Deliver schema evidence\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );

    tasks::manage(
        workspace.root(),
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/010-scheme.md"
        }))
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-scheme:schema";
    let claim = tasks::claim(
        workspace.root(),
        tasks::Claim {
            task_id: task_id.into(),
            stage: "schema_input".into(),
            worker_id: "schema-author".into(),
            worktree: workspace.root().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        claim["context_bundle"]["proof_schema"],
        json!({
            "type": "object",
            "properties": {
                "report": {
                    "type": "object",
                    "properties": {
                        "entries": {
                            "type": "array",
                            "properties": {},
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "properties": {},
                                        "required": [],
                                        "additionalProperties": null,
                                        "items": null,
                                "enum": ["accepted", "rejected"]
                                    }
                                },
                                "required": ["name"],
                                "additionalProperties": false,
                                "items": null,
                                "enum": []
                            },
                            "required": [],
                            "additionalProperties": null,
                            "enum": []
                        }
                    },
                    "required": ["entries"],
                    "additionalProperties": false,
                    "items": null,
                    "enum": []
                }
            },
            "required": ["report"],
            "additionalProperties": false,
            "items": null,
            "enum": []
        })
    );

    let properties = claim["context_bundle"]["proof_schema"]
        .pointer("/properties/report/properties/entries/items/additionalProperties")
        .expect("array item schema preserves the forbidden-property contract");
    assert_eq!(properties, false);

    let task: contextunity_forge_mcp::db::tasks_store::Task = {
        let store = tasks::store(workspace.root()).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&descriptor).unwrap()
    };
    let before_state = || {
        let store = tasks::store(workspace.root()).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        let current: contextunity_forge_mcp::db::tasks_store::Task =
            serde_json::from_str(&descriptor).unwrap();
        let gates = store
            .connection
            .prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY gate,revision")
            .unwrap()
            .query_map([task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let claims = store
            .connection
            .prepare("SELECT revision,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")
            .unwrap()
            .query_map([task_id], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let submissions: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_submissions WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        (
            current.status,
            current.stage,
            current.claim_revision,
            current.worker_id,
            gates,
            claims,
            submissions,
        )
    };
    assert_eq!(task.stage, "schema_input");
    let unchanged = before_state();
    let invalid_proofs = [
        ("nested required field missing", json!({"report":{}})),
        (
            "wrong nested type",
            json!({"report":{"entries":"not-an-array"}}),
        ),
        (
            "enum value rejected",
            json!({"report":{"entries":[{"name":"unknown"}]}}),
        ),
        (
            "array element type rejected",
            json!({"report":{"entries":[42]}}),
        ),
        (
            "forbidden nested property rejected",
            json!({"report":{"entries":[],"extra":true}}),
        ),
    ];
    for (case, proof) in invalid_proofs {
        let mut invalid_evidence = evidence(&task);
        invalid_evidence.proof = json!({"scheme_proof":proof});
        let error = tasks::submit(
            workspace.root(),
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": "schema_input",
                "action": "pass",
                "evidence": invalid_evidence
            }))
            .unwrap(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("TASK_EVIDENCE_INVALID"),
            "{case} produced an unexpected error: {error:#}"
        );
        assert_eq!(
            before_state(),
            unchanged,
            "{case} mutated task/gate/claim state"
        );
    }

    let mut accepted_evidence = evidence(&task);
    accepted_evidence.proof = json!({
        "scheme_proof":{"report":{"entries":[{"name":"accepted"}]}}
    });
    let accepted = tasks::submit(
        workspace.root(),
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "schema_input",
            "action": "pass",
            "evidence": accepted_evidence
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(accepted["stage"], "review");
    let submitted = before_state();
    assert_eq!(submitted.0, "ready");
    assert_eq!(submitted.1, "review");
    assert_eq!(submitted.6, unchanged.6 + 1);
    let stored_gate: (String, Option<String>) = tasks::store(workspace.root())
        .unwrap()
        .connection
        .query_row(
            "SELECT state,evidence FROM task_gates WHERE task_id=?1 AND gate='schema_input' ORDER BY revision DESC LIMIT 1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_gate.0, "passed");
    let stored_evidence: Value = serde_json::from_str(stored_gate.1.as_deref().unwrap()).unwrap();
    assert_eq!(
        stored_evidence["proof"]["scheme_proof"]["report"]["entries"][0]["name"],
        "accepted"
    );
}

#[test]
#[cfg(unix)]
fn task_profile_resolution_rejects_file_and_parent_symlink_escapes() {
    use sha2::Digest;
    use std::os::unix::fs::symlink;

    let external = Workspace::new();
    let profile = "roles:\n  independent_reviewer:\n    model: outside-workspace\n";
    external.write("profiles/team.yaml", profile);
    let digest = format!("{:x}", sha2::Sha256::digest(profile.as_bytes()));

    for (name, reference, link_path) in [
        (
            "file",
            "profile-link.yaml",
            external.root().join("profiles/team.yaml"),
        ),
        (
            "parent",
            "profile-dir/team.yaml",
            external.root().join("profiles"),
        ),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_profile_symlink_{name}"));
        root.write("src/lib.rs", "pub fn profile() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let link = root.0.join(if name == "file" {
            "profile-link.yaml"
        } else {
            "profile-dir"
        });
        symlink(&link_path, &link).unwrap();
        let manifest = format!(
            "---\nid: m-profile-{name}\ntitle: Profile path\ndoc_type: contract\nstatus: active\nacdd_profile: {reference}:{digest}\n---\n# Profile path\n```yaml\ntask_ref: scoped\ntarget: Confine profile resolution\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        );
        root.write("docs/milestones/010-profile.md", &manifest);

        let error = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "sync",
                "milestone_ref": "docs/milestones/010-profile.md"
            }))
            .unwrap(),
        )
        .expect_err("pinned profile symlink escape must fail closed");
        assert!(
            error.to_string().contains(
                "TASK_SCOPE_INVALID: docs/milestones/010-profile.md. symlink escape. Do:"
            ),
            "{name} symlink should be rejected as an escape: {error}"
        );
    }

    let external_profile =
        "roles:\n  contract_author:\n    models:\n      - model: external-profile-escape\n";
    let in_root_profile =
        "roles:\n  contract_author:\n    models:\n      - model: in-root-profile\n";
    let external_default = Workspace::new();
    external_default.write("profiles/profile.yaml", external_profile);
    let mut automatic_default_escapes = Vec::new();

    for (name, link_kind, expected_model) in [
        ("default_file", "file", None),
        ("default_parent", "parent", None),
        ("default_local", "local", Some("in-root-profile")),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_automatic_profile_{name}"));
        root.write("src/lib.rs", "pub fn profile() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nagents_guidance: AGENTS.md\n",
        );
        root.write(
            "AGENTS.md",
            "# Agent instructions\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);

        match link_kind {
            "file" => {
                std::fs::create_dir_all(root.0.join(".forge/acdd")).unwrap();
                symlink(
                    external_default.root().join("profiles/profile.yaml"),
                    root.0.join(".forge/acdd/profile.yaml"),
                )
                .unwrap();
            }
            "parent" => {
                std::fs::create_dir_all(root.0.join(".forge")).unwrap();
                symlink(
                    external_default.root().join("profiles"),
                    root.0.join(".forge/acdd"),
                )
                .unwrap();
            }
            "local" => root.write(".forge/acdd/profile.yaml", in_root_profile),
            _ => unreachable!("profile case is declared in the table"),
        }

        root.write(
            "docs/milestones/010-profile.md",
            "---\nid: m-automatic-profile\ntitle: Automatic profile\ndoc_type: contract\nstatus: active\n---\n# Automatic profile\n```yaml\ntask_ref: scoped\ntarget: Confine automatic profile resolution\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
        );

        let sync = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "sync",
                "docs/milestones/010-profile.md",
            ])
            .output()
            .unwrap();

        if let Some(expected_model) = expected_model {
            assert!(
                sync.status.success(),
                "ordinary in-root default profile should load: {}",
                String::from_utf8_lossy(&sync.stderr)
            );
            let synced: Value = serde_json::from_slice(&sync.stdout).unwrap();
            let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
            let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--root",
                    root.0.to_str().unwrap(),
                    "task",
                    "inspect",
                    task_id,
                ])
                .output()
                .unwrap();
            assert!(
                inspect.status.success(),
                "ordinary in-root task inspection should succeed: {}",
                String::from_utf8_lossy(&inspect.stderr)
            );
            let inspected: Value = serde_json::from_slice(&inspect.stdout).unwrap();
            assert!(inspected["workflow_guidance"]["role_guide"]
                .as_str()
                .unwrap()
                .contains(expected_model));
        } else if sync.status.success() {
            let synced: Value = serde_json::from_slice(&sync.stdout).unwrap();
            let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
            let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--root",
                    root.0.to_str().unwrap(),
                    "task",
                    "inspect",
                    task_id,
                ])
                .output()
                .unwrap();
            let exposed_model = if inspect.status.success() {
                let inspected: Value = serde_json::from_slice(&inspect.stdout).unwrap();
                inspected["workflow_guidance"]["role_guide"]
                    .as_str()
                    .map(str::to_owned)
            } else {
                None
            };
            automatic_default_escapes.push(format!(
                "{name}: sync succeeded, inspect exit={}, exposed model={exposed_model:?}, stderr={}",
                inspect.status,
                String::from_utf8_lossy(&inspect.stderr)
            ));
        } else {
            assert!(
                String::from_utf8_lossy(&sync.stderr).contains(
                    "TASK_SCOPE_INVALID: docs/milestones/010-profile.md. symlink escape. Do:"
                ),
                "automatic default {name} symlink should fail as an escape: {}",
                String::from_utf8_lossy(&sync.stderr)
            );
        }
    }
    assert!(
        automatic_default_escapes.is_empty(),
        "automatic default profile symlink escapes: {automatic_default_escapes:?}"
    );
}

#[test]
fn linked_pinned_tasks_ignore_linked_workspace_profile_overrides() {
    use sha2::Digest;
    let task_profile = "roles:\n  builder:\n    models:\n      - model: linked-task-override\n";
    let digest = format!("{:x}", sha2::Sha256::digest(task_profile.as_bytes()));
    let invalid_linked_base = "gates:\n  - id: linked_only\n    role: builder\n    proof: none\n";

    for (case, linked_base_path) in [
        ("forge_config", "forge-mcp.yaml"),
        ("repository_profile", ".forge/acdd/profile.yaml"),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_linked_profile_{case}"));
        root.write("src/lib.rs", "pub fn primary() {}\n");
        root.write("linked/src/lib.rs", "pub fn linked() {}\n");
        root.write("linked/AGENTS.md", "# Linked instructions\n");
        root.write("linked/profiles/task.yaml", task_profile);
        if linked_base_path == "forge-mcp.yaml" {
            root.write("linked/invalid-base.yaml", invalid_linked_base);
            root.write(
                "linked/forge-mcp.yaml",
                "task_repository: linked-repository\ntask_project: linked-project\nacdd_profile: invalid-base.yaml\n",
            );
        } else {
            root.write(
                "linked/forge-mcp.yaml",
                "task_repository: linked-repository\ntask_project: linked-project\n",
            );
            root.write(&format!("linked/{linked_base_path}"), invalid_linked_base);
        }
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nlinked_workspaces:\n  - name: linked\n    path: linked\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: AGENTS.md\n",
        );
        let manifest = format!(
            "---\nid: m-linked-{case}\ntitle: Linked profile\ndoc_type: contract\nstatus: active\n---\n# Linked profile\n```yaml\ntask_ref: pinned\ntarget: Ignore linked base profile\nacdd_profile: profiles/task.yaml:{}\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
            &digest[..16]
        );
        root.write("linked/contracts/010-pinned.md", &manifest);

        let imported = tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","workspace":"linked"})).unwrap(),
        )
        .unwrap_or_else(|error| panic!("{case} linked base profile leaked into task: {error}"));
        let imported_tasks = imported["tasks"].as_array().unwrap();
        assert_eq!(imported_tasks.len(), 1);
        let task_id = imported_tasks[0]["task_id"].as_str().unwrap();
        assert!(task_id.starts_with("linked-project/"));

        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"inspect","task_id":task_id})).unwrap(),
        )
        .unwrap_or_else(|error| {
            panic!("{case} pinned task inspection used linked base profile: {error}")
        });
    }
}

#[test]
fn commit_scope_validation_checks_exact_milestone_and_non_empty_scope() {
    use contextunity_forge_mcp::core::tasks::commit_files_within_scope;

    let milestone_ref = "docs/milestones/010-repack.md";
    let scope = vec!["src/lib.rs".to_string()];

    // Exact milestone file matches
    assert!(commit_files_within_scope(
        &["docs/milestones/010-repack.md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));

    // Suffixes or file basenames in root must NOT match milestone_ref
    assert!(!commit_files_within_scope(
        &["010-repack.md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));
    assert!(!commit_files_within_scope(
        &[
            "notdocs/milestones/010-repack.md".into(),
            "src/lib.rs".into()
        ],
        &scope,
        Some(milestone_ref)
    ));
    assert!(!commit_files_within_scope(
        &["md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));

    // Normalized variants of the exact same path do match
    assert!(commit_files_within_scope(
        &["./docs/milestones/010-repack.md".into()],
        &[],
        Some(milestone_ref)
    ));
    assert!(commit_files_within_scope(
        &["docs\\milestones\\010-repack.md".into()],
        &[],
        Some(milestone_ref)
    ));

    // Protected paths under .forge/acdd/** and .forge/frameworks/** are NOT treated as runtime files:
    // they require explicit scope admission.
    assert!(!commit_files_within_scope(
        &[".forge/acdd/profile.yaml".into()],
        &scope,
        Some(milestone_ref),
    ));
    assert!(!commit_files_within_scope(
        &[".forge/frameworks/rules.yaml".into()],
        &scope,
        Some(milestone_ref),
    ));
    // If explicitly admitted in task scope, they pass:
    assert!(commit_files_within_scope(
        &[".forge/acdd/profile.yaml".into()],
        &[".forge/acdd/profile.yaml".into()],
        Some(milestone_ref),
    ));

    // Internal runtime artifacts (sqlite, lock, log, tasks runtime, checkpoints) pass without scope:
    assert!(commit_files_within_scope(
        &[".forge/tasks.sqlite".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/tasks.lock".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/checkpoints.json".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/tasks/state.sqlite".into()],
        &[],
        Some(milestone_ref),
    ));
}

#[test]
fn validate_commit_for_task_enforces_scope_and_ancestry_invariants() {
    use contextunity_forge_mcp::core::tasks::validate_commit_for_task;

    let root = ScopedWorkspace::new("forge_validate_commit_invariants");
    root.write("src/lib.rs", "pub fn base() {}\n");

    // Commit 1: base commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "src/lib.rs",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Baseline commit",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let base_sha = git_head(&root.0);

    // Commit 2: intermediate commit C1
    root.write("src/lib.rs", "pub fn base() {}\npub fn c1() {}\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Intermediate commit C1",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let c1_sha = git_head(&root.0);

    // Commit 3: delivery commit D
    root.write(
        "src/lib.rs",
        "pub fn base() {}\npub fn c1() {}\npub fn delivery() {}\n",
    );
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Delivery commit D",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let d_sha = git_head(&root.0);

    let scope = vec!["src/lib.rs".to_string()];
    let milestone_ref = "docs/milestones/010-test.md";

    // 1. Delivery commit D is valid when accepted_delivery is D
    assert!(validate_commit_for_task(
        &root.0,
        &d_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha)
    )
    .is_ok());

    // 2. Intermediate commit C1 is REJECTED when accepted_delivery is D (C1 is an ancestor of D)
    let c1_err = validate_commit_for_task(
        &root.0,
        &c1_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        c1_err.to_string().contains("TASK_COMMIT_OBSOLETE"),
        "expected obsolete commit error, got: {c1_err}"
    );

    // 3. Baseline commit is REJECTED (does not advance beyond baseline)
    let base_err = validate_commit_for_task(
        &root.0,
        &base_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        base_err.to_string().contains("TASK_COMMIT_OBSOLETE"),
        "expected obsolete baseline error, got: {base_err}"
    );

    // 4. Empty scope rejects commits that touch files outside scope (such as src/lib.rs):
    let outside_err = validate_commit_for_task(
        &root.0,
        &d_sha,
        &[],
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        outside_err
            .to_string()
            .contains("TASK_COMMIT_OUTSIDE_SCOPE"),
        "expected outside scope error for empty scope on src/lib.rs, got: {outside_err}"
    );

    // 5. Commits that only modify the milestone doc (or runtime files) fail with TASK_COMMIT_IGNORES_SCOPE
    root.write("docs/milestones/010-test.md", "# Milestone\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "docs/milestones/010-test.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Milestone doc only",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let doc_only_sha = git_head(&root.0);
    let empty_scope_err = validate_commit_for_task(
        &root.0,
        &doc_only_sha,
        &[],
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        empty_scope_err
            .to_string()
            .contains("TASK_COMMIT_IGNORES_SCOPE"),
        "expected ignores scope error for empty scope, got: {empty_scope_err}"
    );

    // 6. Divergent commit with different changes in scope MUST fail with TASK_COMMIT_TREE_MISMATCH
    root.write(
        "src/lib.rs",
        "pub fn base() {}\npub fn malicious_divergent() {}\n",
    );
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Divergent commit with different changes in scope",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let d_alt_sha = git_head(&root.0);
    let mismatch_err = validate_commit_for_task(
        &root.0,
        &d_alt_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        mismatch_err
            .to_string()
            .contains("TASK_COMMIT_TREE_MISMATCH"),
        "expected tree mismatch error for divergent commit, got: {mismatch_err}"
    );

    // 7. git_scoped_tree_changed compares full tree when scope is empty
    assert!(
        contextunity_forge_mcp::core::git::git_scoped_tree_changed(
            &root.0,
            &d_sha,
            &d_alt_sha,
            &[]
        )
        .unwrap(),
        "git_scoped_tree_changed with empty scope must detect full tree difference"
    );
    assert!(
        !contextunity_forge_mcp::core::git::git_scoped_tree_changed(&root.0, &d_sha, &d_sha, &[])
            .unwrap(),
        "git_scoped_tree_changed with empty scope must return false for identical commits"
    );
}

#[test]
fn inline_role_mode_adapts_guidance_text() {
    let root = ScopedWorkspace::new("forge_inline_role_mode");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(
        "docs/AGENTS.md",
        "# Agent Guidance\nUse ContextUnity Forge.\n",
    );
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let profile_yaml = r#"
gates:
  - id: contract
    role: contract_author
    proof: contract
  - id: build
    role: builder
    proof: command
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    review_sources: [review]
    reject_to: build
roles:
  independent_reviewer:
    mode: inline
    reuse_on_reject: true
    guidance: "Spawn a dedicated reviewer subagent to review candidate diff across contours."
"#;
    root.write("custom_profile.yaml", profile_yaml);

    let milestone_content = r#"---
id: m-inline-role
title: Inline role test
doc_type: contract
status: active
---
# Inline role test

```yaml
task_ref: inline-task
target: Verify inline role guidance
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-inline.md", milestone_content);

    let sync_res = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-inline.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    let inspected = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "inspect",
            "task_id": task_id
        }))
        .unwrap(),
    )
    .unwrap();
    // At contract stage, mode is subagent
    assert_eq!(inspected["workflow_guidance"]["mode"], "subagent");

    // Advance to review stage
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "contract".into(),
            worker_id: "author".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut claimed_task = store.inspect(task_id).unwrap();
    let evidence_val = evidence(&claimed_task);
    contextunity_forge_mcp::engine::tasks::submit(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "contract",
            "action": "pass",
            "evidence": evidence_val
        }))
        .unwrap(),
    )
    .unwrap();

    contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "build".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    claimed_task = store.inspect(task_id).unwrap();
    let build_evidence = evidence(&claimed_task);
    contextunity_forge_mcp::engine::tasks::submit(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "build",
            "action": "pass",
            "evidence": build_evidence
        }))
        .unwrap(),
    )
    .unwrap();

    // Now claim review stage: mode must be inline and guidance must be adapted!
    let review_claim = contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "review".into(),
            worker_id: "inline-reviewer".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let guidance = &review_claim["workflow_guidance"];
    assert_eq!(guidance["role"], "independent_reviewer");
    assert_eq!(guidance["mode"], "inline");
    let role_guide = guidance["role_guide"].as_str().unwrap();
    assert!(
        role_guide.contains(
            "Execute inline in the current agent to review candidate diff across contours."
        ),
        "expected adapted inline role guide, got: {role_guide}"
    );
    assert!(!role_guide.contains("Spawn a dedicated"));
}

#[test]
fn cross_milestone_dependency_diagnostic_gates_readiness_and_claim() {
    let root = ScopedWorkspace::new("forge_cross_milestone");
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let m1_text = "---\nid: m-first\ntitle: First\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# First\n```yaml\ntask_ref: producer\ntarget: Deliver producer\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let m2_text = "---\nid: m-second\ntitle: Second\ndoc_type: contract\nstatus: planned\ndepends_on: [m-first]\ninvariants: [isolated]\n---\n# Second\n```yaml\ntask_ref: consumer\ntarget: Deliver consumer\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [\"m-first:producer\"]\n```\n";
    let m1 = Milestone::parse(m1_text, "forge-mcp").unwrap();
    let m2 = Milestone::parse(m2_text, "forge-mcp").unwrap();
    root.write("src/lib.rs", "pub fn f() {}\n");
    root.write("docs/milestones/m1.md", m1_text);
    root.write("docs/milestones/m2.md", m2_text);

    store.sync(&m1, "docs/milestones/m1.md", &root.0).unwrap();
    store.sync(&m2, "docs/milestones/m2.md", &root.0).unwrap();

    let producer_id = m1.task_id(&m1.tasks[0]);
    let consumer_id = m2.task_id(&m2.tasks[0]);

    let tasks = store.list(None, "all", None).unwrap();
    let producer_task = tasks.iter().find(|t| t.task_id == producer_id).unwrap();
    let consumer_task = tasks.iter().find(|t| t.task_id == consumer_id).unwrap();
    assert_eq!(producer_task.status, "ready");
    assert_eq!(consumer_task.status, "blocked");

    // Persist the computed blocked state so the claim seam can exercise status precedence.
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'blocked') WHERE task_id = ?1",
            [&consumer_id],
        )
        .unwrap();

    // Attempting to claim consumer should fail because its status is not ready.
    let worktree = root.0.to_string_lossy().into_owned();
    let status_err = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap_err();
    let status_message = status_err.to_string();

    // Exercise the dependency-specific diagnostic after status becomes ready.
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'ready') WHERE task_id = ?1",
            [&consumer_id],
        )
        .unwrap();
    let dependency_err = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap_err();
    let dependency_message = dependency_err.to_string();
    let expected_status_message = format!(
        "TASK_NOT_READY: task '{consumer_id}'. status is 'blocked' (active stage 'contract'). Do: claim only ready tasks at their active stage."
    );
    let expected_dependency_message = format!(
        "TASK_NOT_READY: task '{consumer_id}' has unsatisfied dependencies: [{producer_id}]. Do: complete dependencies before claiming stage 'contract'."
    );
    assert!(
        status_message == expected_status_message
            && dependency_message == expected_dependency_message,
        "unexpected TASK_NOT_READY diagnostics; status: {status_message:?}; dependencies: {dependency_message:?}"
    );

    // Complete producer task
    store.connection.execute(
        "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'completed') WHERE task_id = ?1",
        [&producer_id],
    ).unwrap();

    // Now consumer should be ready!
    let tasks_after = store.list(None, "all", None).unwrap();
    let consumer_after = tasks_after
        .iter()
        .find(|t| t.task_id == consumer_id)
        .unwrap();
    assert_eq!(consumer_after.status, "ready");

    // And claiming consumer should now succeed!
    let claimed = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap();
    assert_eq!(claimed.task_id, consumer_id);
    assert_eq!(claimed.status, "in_progress");
}

#[test]
fn task_manage_selector_diagnostics_report_fields_before_resolution() {
    let root = ScopedWorkspace::new("forge_selector_diagnostics");
    let cases = [
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                ..Default::default()
            },
            "inspect",
            "task_id",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                task_id: Some("forge-mcp/m-missing:task".into()),
                milestone_ref: Some("docs/010-test.md".into()),
                ..Default::default()
            },
            "inspect",
            "milestone_ref",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                ..Default::default()
            },
            "sync",
            "milestone_ref, workspace",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::ExtendScope,
                task_id: Some("forge-mcp/m-test:first".into()),
                paths: Some(Vec::new()),
                ..Default::default()
            },
            "extend_scope",
            "paths",
        ),
    ];

    for (request, action, fields) in cases {
        let selector = request
            .task_id
            .as_deref()
            .or(request.milestone_ref.as_deref())
            .unwrap_or("unselected")
            .to_owned();
        let error = tasks::manage(&root.0, request).unwrap_err().to_string();
        assert_eq!(
            error,
            format!(
                "TASK_SELECTOR_INVALID: {selector}. action '{action}'. missing or invalid fields: [{fields}]. Do: pass required fields for '{action}'."
            )
        );
    }
}

#[test]
fn task_store_diagnostics_cover_not_found_stale_claim_and_submission_precedence() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let missing_id = "forge-mcp/m-test:missing";
    let expected_not_found = format!(
        "TASK_NOT_FOUND: task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
    );
    assert_eq!(
        store.inspect(missing_id).unwrap_err().to_string(),
        expected_not_found
    );
    let not_found_evidence = Evidence {
        task_id: missing_id.into(),
        stage: "contract".into(),
        claim_revision: 1,
        contract_revision: 1,
        worker_id: "diagnostic-builder".into(),
        worktree: root.0.to_string_lossy().into_owned(),
        commit: None,
        candidate_baseline_head: None,
        proof: json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}}),
    };
    assert_eq!(
        store
            .claim(
                missing_id,
                "contract",
                "diagnostic-builder",
                &not_found_evidence.worktree
            )
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store.reset(missing_id).unwrap_err().to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .release_failed_claim(missing_id, 1)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .extend_scope(missing_id, &["src/other.rs".into()], &root.0)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .submit(missing_id, "contract", &not_found_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: missing_id.into(),
                stage: "contract".into(),
                worker_id: "diagnostic-builder".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_NOT_FOUND: {missing_id}. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
        )
    );
    for action in [
        tasks::ManageAction::Inspect,
        tasks::ManageAction::ExtendScope,
    ] {
        let error = tasks::manage(
            &root.0,
            tasks::Manage {
                action,
                task_id: Some(missing_id.into()),
                paths: (action == tasks::ManageAction::ExtendScope)
                    .then(|| vec!["src/other.rs".into()]),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            error,
            format!(
                "TASK_NOT_FOUND: {missing_id}. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
            )
        );
    }
    assert_eq!(
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Create,
                milestone_ref: Some("docs/010-test.md".into()),
                task_ref: Some("missing".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_NOT_FOUND: docs/010-test.md. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
        )
    );

    let ready = store.inspect(&id).unwrap();
    assert_eq!(
        store
            .release_failed_claim(&id, ready.claim_revision)
            .unwrap_err()
            .to_string(),
        format!("TASK_STALE_CLAIM: task '{id}'. status is 'ready'. Do: leave the claim unchanged.")
    );

    let worktree = root.0.to_string_lossy().into_owned();
    let claimed = store
        .claim(&id, "contract", "diagnostic-builder", &worktree)
        .unwrap();
    let wrong_revision = claimed.claim_revision + 1;
    assert_eq!(
        store
            .release_failed_claim(&id, wrong_revision)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_CLAIM: task '{id}'. active claim_revision {} != requested {wrong_revision}. Do: release claim_revision {}.",
            claimed.claim_revision, claimed.claim_revision
        )
    );
    store
        .connection
        .execute(
            "UPDATE task_claims SET ended = 1 WHERE task_id = ?1 AND revision = ?2",
            rusqlite::params![id, claimed.claim_revision],
        )
        .unwrap();
    assert_eq!(
        store
            .release_failed_claim(&id, claimed.claim_revision)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_CLAIM: task '{id}'. claim_revision {} has no open claim row. Do: claim the task again before modifying it.",
            claimed.claim_revision
        )
    );
    store.reset(&id).unwrap();
    drop(store);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let mut active = store
        .claim(&id, "contract", "diagnostic-builder", &worktree)
        .unwrap();
    let mut stage_evidence = evidence(&active);
    stage_evidence.stage = "build".into();
    assert_eq!(
        store
            .submit(&id, "contract", &stage_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STAGE_INVALID: task '{id}'. evidence.stage 'build' != submitted stage 'contract'. Do: set evidence.stage to 'contract'."
        )
    );

    let mut empty_commit = evidence(&active);
    empty_commit.commit = Some(" \t".into());
    assert_eq!(
        store
            .submit(&id, "contract", &empty_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. commit field is empty. Do: set commit to candidate snapshot unavailable."
        )
    );

    for (value, expected) in [
        (
            Value::Null,
            format!(
                "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence is not a valid JSON object. Do: pass evidence as a JSON object."
            ),
        ),
        (
            json!({}),
            format!(
                "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence JSON does not match the evidence object. Do: pass the evidence fields required by stage 'contract'."
            ),
        ),
    ] {
        assert_eq!(
            tasks::submit(
                &root.0,
                tasks::Submit {
                    task_id: id.clone(),
                    stage: "contract".into(),
                    action: tasks::Action::Pass,
                    evidence: value,
                    findings: None,
                }
            )
            .unwrap_err()
            .to_string(),
            expected
        );
    }
    let mut submitted_evidence = evidence(&active);
    submitted_evidence.stage = "build".into();
    assert_eq!(
        store
            .submit(&id, "build", &submitted_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STAGE_INVALID: task '{id}'. submitted stage 'build' does not match active stage 'contract'. Do: submit for active stage 'contract'."
        )
    );

    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'ready') WHERE task_id = ?1",
            [&id],
        )
        .unwrap();
    let ready_with_claim = store.inspect(&id).unwrap();
    let ready_evidence = evidence(&ready_with_claim);
    assert_eq!(
        store
            .submit(&id, "contract", &ready_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_SUBMISSION: task '{id}'. status is 'ready'. Do: claim the task before submitting."
        )
    );
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'in_progress') WHERE task_id = ?1",
            [&id],
        )
        .unwrap();
    active = store.inspect(&id).unwrap();

    let active_revision = active.claim_revision;
    let active_contract_revision = active.contract_revision;
    let active_worker = active.worker_id.clone().unwrap();
    let active_worktree = active.worktree.clone().unwrap();
    let cases = vec![
        {
            let mut supplied = evidence(&active);
            supplied.claim_revision += 1;
            (
                "claim_revision",
                supplied.claim_revision.to_string(),
                active_revision.to_string(),
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.contract_revision += 1;
            (
                "contract_revision",
                supplied.contract_revision.to_string(),
                active_contract_revision.to_string(),
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.task_id = "forge-mcp/m-other:task".into();
            ("task_id", supplied.task_id.clone(), id.clone(), supplied)
        },
        {
            let mut supplied = evidence(&active);
            supplied.worker_id = "other-worker".into();
            (
                "worker_id",
                supplied.worker_id.clone(),
                active_worker,
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.worktree = "/other/worktree".into();
            (
                "worktree",
                supplied.worktree.clone(),
                active_worktree,
                supplied,
            )
        },
    ];
    for (field, evidence_value, active_value, supplied) in cases {
        assert_eq!(
            store
                .submit(&id, "contract", &supplied, "pass", None)
                .unwrap_err()
                .to_string(),
            format!(
                "TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_value} != active {active_value}. Do: resubmit with active {field} {active_value}."
            )
        );
    }

    let mut invalid_proof = evidence(&active);
    invalid_proof.proof = Value::Null;
    assert_eq!(
        store
            .submit(&id, "contract", &invalid_proof, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. proof is null. Do: provide non-empty proof for stage 'contract'."
        )
    );

    let accepted = evidence(&active);
    store
        .submit(&id, "contract", &accepted, "pass", None)
        .unwrap();
    let mut different = accepted;
    different.proof =
        json!({"contract_proof":{"seam_test_ref":"tests/acdd/other.rs","red_exit_code":101}});
    assert_eq!(
        store
            .submit(&id, "contract", &different, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.",
            active.claim_revision, active.claim_revision
        )
    );
}

#[test]
fn delivery_rejects_commit_that_differs_from_candidate_when_auto_commit_is_disabled() {
    let root = ScopedWorkspace::new("forge_delivery_candidate_diagnostic");
    root.write("src/lib.rs", "pub fn candidate() -> bool { true }\n");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    root.write(
        "custom_profile.yaml",
        r#"
gates:
  - id: contract
    role: contract_author
    proof: contract
  - id: build
    role: builder
    proof: command
    sha_snapshot: true
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    review_sources: [review]
    reject_to: build
    auto_commit: false
"#,
    );
    root.write(
        "docs/010-candidate.md",
        "---\nid: m-candidate\ntitle: Candidate diagnostics\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Candidate diagnostics\n```yaml\ntask_ref: candidate\ntarget: Verify delivery candidate diagnostics\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("docs/010-candidate.md".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let task_id = "forge-mcp/m-candidate:candidate";
    for (stage, worker) in [
        ("contract", "author"),
        ("build", "builder"),
        ("review", "reviewer"),
    ] {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.into(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: stage.into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(evidence(&task)).unwrap(),
                findings: None,
            },
        )
        .unwrap();
    }
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: "delivery".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut store = tasks::store_for_task(&root.0, task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let (candidate_snapshot, baseline) = store.build_snapshot_candidate(task_id).unwrap().unwrap();
    let mut supplied = evidence(&task);
    supplied.commit = Some("caller-selected-commit".into());
    supplied.candidate_baseline_head = Some(baseline);
    assert_eq!(
        store
            .submit(task_id, "deliver", &supplied, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}."
        )
    );
}

#[test]
fn greenfield_seam_diagnostic_reports_missing_scope_files_and_scaffolding_guidance() {
    let root = prepare_indexed_boundary_workspace(
        "pub fn route() -> usize { 1 }\n",
        "",
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
        "route",
        None,
        None,
    );
    std::fs::remove_file(root.0.join("src/api.rs")).unwrap();
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let task_id = "forge-mcp/m-current-boundary:routing";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "contract",
            "worker_id": "contract-author",
            "worktree": root.0
        }))
        .unwrap(),
    )
    .unwrap();
    let mut store = tasks::store_for_task(&root.0, task_id).unwrap();
    let claimed = store.inspect(task_id).unwrap();
    let mut proof = evidence(&claimed);
    proof.proof = json!({
        "contract_proof": {
            "seam_test_ref": "src/seam.rs::test_connected",
            "red_exit_code": 101
        }
    });

    for phase in ["missing", "stub_unindexed", "stub_indexed"] {
        if phase == "stub_unindexed" {
            root.write("src/api.rs", "pub fn route() -> usize { 0 }\n");
        } else if phase == "stub_indexed" {
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        }
        let result = store.submit(task_id, "contract", &proof, "pass", None);
        if phase == "stub_indexed" {
            result.expect("indexed real production stub must permit red contract proof");
            assert_eq!(store.inspect(task_id).unwrap().stage, "build");
            continue;
        }
        let error = result.unwrap_err().to_string();
        assert!(error.contains("TASK_SEAM_DISCONNECTED"), "{phase}: {error}");
        if phase == "missing" {
            assert!(
                error.contains("target scope files do not exist yet on disk (src/api.rs)"),
                "{error}"
            );
            assert!(error.contains("create minimal file scaffolding"), "{error}");
            assert!(error.contains("contextunity-forge-mcp build"), "{error}");
        } else {
            assert!(!error.contains("do not exist yet"), "{error}");
            assert!(
                error.contains("refresh the claimed-worktree code map"),
                "{error}"
            );
        }
        let rejected = store.inspect(task_id).unwrap();
        assert_eq!(rejected.stage, "contract", "{phase}");
        assert_eq!(rejected.claim_revision, claimed.claim_revision, "{phase}");
    }
}
