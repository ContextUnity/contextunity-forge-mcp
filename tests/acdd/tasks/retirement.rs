use super::*;

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
