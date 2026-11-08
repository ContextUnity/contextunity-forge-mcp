use super::super::snapshot::FixtureSnapshot;
use super::*;
use crate::acdd::tasks::harness::TaskHarness;

const RETIREMENT_TASK: &str = "forge-mcp/m-current-boundary:routing";
const RETIREMENT_SEAM: &str = "src/seam.rs::test_connected";
const RETIREMENT_MANIFEST: &str = "docs/milestones/010-boundary.md";

fn retirement_workspace(external_caller: bool) -> Workspace {
    prepare_indexed_boundary_workspace(
        "pub fn route() -> usize { 1 }\n",
        if external_caller {
            "use crate::api::route;\npub fn invoke() -> usize { route() }\n"
        } else {
            ""
        },
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
        "route",
        None,
        None,
        RETIREMENT_SEAM,
    )
}

fn check_file_retirement_groups(groups: &[(bool, bool, &[&str])]) {
    // Each distinct caller/scope combination has one contract and build claim;
    // restore that real state for each staging, refresh, and retirement case.
    for &(external_caller, admit_caller, cases) in groups {
        let root = retirement_workspace(external_caller);
        if admit_caller {
            tasks::manage(root.root(), serde_json::from_value(json!({"action":"extend_scope", "task_id":RETIREMENT_TASK, "paths":["src/caller.rs"]})).unwrap()).unwrap();
        }
        TaskHarness::claim_at(root.root(), RETIREMENT_TASK, "contract", "contract-author").unwrap();
        submit_indexed_boundary(&root, "contract", RETIREMENT_SEAM).unwrap();
        TaskHarness::claim_at(root.root(), RETIREMENT_TASK, "build", "builder").unwrap();

        // extend_scope updates a tracked, uncommitted manifest. Preserve its
        // exact contents and mode separately from the committed Git baseline.
        let extended_manifest = admit_caller.then(|| {
            let path = root.path(RETIREMENT_MANIFEST);
            (
                std::fs::read(&path).unwrap(),
                std::fs::metadata(path).unwrap().permissions(),
            )
        });
        let snapshot = FixtureSnapshot::capture(&root);

        for &case in cases {
            for staged in [false, true] {
                for refresh in [false, true] {
                    snapshot.restore(&root);
                    if let Some((contents, permissions)) = &extended_manifest {
                        let path = root.path(RETIREMENT_MANIFEST);
                        std::fs::write(&path, contents).unwrap();
                        std::fs::set_permissions(path, permissions.clone()).unwrap();
                    }
                    root.remove("src/api.rs");
                    if case == "admitted-caller" {
                        root.write("src/caller.rs", "pub fn invoke() -> usize { 0 }\n");
                    }
                    if matches!(case, "removed-caller" | "unadmitted-removed-caller") {
                        root.remove("src/caller.rs");
                    }
                    if staged {
                        let output = TaskHarness::stage_tracked_changes(root.root(), &["src"]);
                        assert!(output.status.success());
                    }
                    if refresh {
                        root.build_to(root.path(".forge/code-map.sqlite"));
                    }
                    let result = submit_indexed_boundary(&root, "build", RETIREMENT_SEAM);
                    if matches!(case, "external-live" | "unadmitted-removed-caller") {
                        let error = result.unwrap_err().to_string();
                        assert!(
                            error.contains("TASK_SCOPE_INCOMPLETE")
                                && error.contains("src/caller.rs")
                                && error.contains("route"),
                            "{case}/{staged}/{refresh}: {error}"
                        );
                        let task = TaskHarness::manage_task(
                            root.root(),
                            RETIREMENT_TASK,
                            tasks::ManageAction::Inspect,
                        )
                        .unwrap();
                        assert_eq!(task["stage"], "build");
                        assert_eq!(task["status"], "in_progress");
                    } else {
                        assert!(result.is_ok(), "{case}/{staged}/{refresh}: {result:?}");
                        assert_eq!(result.unwrap()["stage"], "review");
                    }
                }
            }
        }
    }
}

#[test]
fn build_gate_handles_file_retirement_and_preserves_caller_safety() {
    check_file_retirement_groups(&[
        (false, false, &["no-callers"]),
        (true, false, &["external-live", "unadmitted-removed-caller"]),
    ]);
}

#[test]
fn build_gate_handles_file_retirement_with_admitted_caller_scope() {
    check_file_retirement_groups(&[(true, true, &["admitted-caller", "removed-caller"])]);
}

#[test]
fn contract_rejects_retirement_without_baseline_caller_evidence() {
    for external_caller in [false, true] {
        let root = retirement_workspace(external_caller);
        tasks::manage(root.root(), serde_json::from_value(json!({"action":"extend_scope", "task_id":RETIREMENT_TASK, "paths":["src/other.rs"]})).unwrap()).unwrap();
        root.write("src/seam.rs", "use crate::other::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 2); }\n");
        commit_fixture_baseline(&root);
        root.build_to(root.path(".forge/code-map.sqlite"));
        let snapshot = FixtureSnapshot::capture(&root);

        for staged in [false, true] {
            for refresh in [false, true] {
                snapshot.restore(&root);
                root.remove("src/api.rs");
                if staged {
                    assert!(TaskHarness::stage_tracked_changes(root.root(), &["src"])
                        .status
                        .success());
                }
                if refresh {
                    root.build_to(root.path(".forge/code-map.sqlite"));
                }
                // Claim after the mutation, exactly as the production scenario
                // does; checkpoint state may depend on staged and indexed files.
                TaskHarness::claim_at(root.root(), RETIREMENT_TASK, "contract", "contract-author")
                    .unwrap();
                let result = submit_indexed_boundary(&root, "contract", RETIREMENT_SEAM);
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
    for external_caller in [false, true] {
        let root = retirement_workspace(external_caller);
        TaskHarness::claim_at(root.root(), RETIREMENT_TASK, "contract", "contract-author").unwrap();
        submit_indexed_boundary(&root, "contract", RETIREMENT_SEAM).unwrap();
        {
            let store = tasks::store_for_task(root.root(), RETIREMENT_TASK).unwrap();
            let (revision, mut result) =
                TaskHarness::latest_submission_result(&store, RETIREMENT_TASK).unwrap();
            for entry in result["boundary_callers"]["entries"]
                .as_array_mut()
                .unwrap()
            {
                entry.as_object_mut().unwrap().remove("target_present");
            }
            TaskHarness::replace_submission_result(&store, RETIREMENT_TASK, revision, &result)
                .unwrap();
        }
        TaskHarness::claim_at(root.root(), RETIREMENT_TASK, "build", "builder").unwrap();
        let snapshot = FixtureSnapshot::capture(&root);

        for indexed_target in [false, true] {
            snapshot.restore(&root);
            root.remove("src/api.rs");
            if !indexed_target {
                root.build_to(root.path(".forge/code-map.sqlite"));
            }
            let result = submit_indexed_boundary(&root, "build", RETIREMENT_SEAM);
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
