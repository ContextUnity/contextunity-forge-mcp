use super::*;

#[test]
fn seam_scope_directory_prefix_treats_sql_wildcards_as_literal() {
    let root = acdd_workspace("src/apiXv1/hidden.rs::test_connected", "");
    root.write("src/lib.rs", "pub mod apiXv1;\npub mod apiXYZv1;\n");
    for actual_dir in ["apiXv1", "apiXYZv1"] {
        root.write(format!("src/{actual_dir}/mod.rs"), "pub mod hidden;\n");
        root.write(
            format!("src/{actual_dir}/hidden.rs"),
            &format!(
                "pub fn route_task() -> usize {{ 1 }}\n#[cfg(test)]\nmod tests {{ use crate::{actual_dir}::hidden::route_task; #[test] fn test_connected() {{ let actual = route_task(); assert_eq!(actual, 1); }} }}\n"
            ),
        );
    }
    for scope_dir in ["api_v1", "api%v1"] {
        root.write(
            format!("src/{scope_dir}/allowed.rs"),
            "pub fn placeholder() {}\n",
        );
    }
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_literal_scope_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/acdd.rs", "#[path = \"acdd/tasks.rs\"]\nmod tasks;\n");
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
    let milestone_ref = "docs/milestones/010-tiered-literal.md";
    let milestone_id = "m-tiered-literal";
    let task_ref = "literal-scope";
    let milestone_source = |scope_dir: &str, contract_revision: u64| {
        format!(
            "---\nid: {milestone_id}\ntitle: Literal directory scope\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: {task_ref}\ntarget: Keep directory scope matching literal\nproof_policy: seam-test-first\ncontract_revision: {contract_revision}\nscope: [src/{scope_dir}/, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n"
        )
    };
    let cases = [
        ("underscore", "api_v1", "apiXv1"),
        ("percent", "api%v1", "apiXYZv1"),
    ];
    root.write(milestone_ref, &milestone_source(cases[0].1, 1));
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    tasks::manage(
        root.root(),
        serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref})).unwrap(),
    )
    .unwrap();
    let snapshot = FixtureSnapshot::capture(&root);
    for (index, (case, scope_dir, actual_dir)) in cases.into_iter().enumerate() {
        if index > 0 {
            snapshot.restore(&root);
            root.write(milestone_ref, &milestone_source(scope_dir, 2));
            tasks::manage(
                root.root(),
                serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref}))
                    .unwrap(),
            )
            .unwrap();
        }
        let task_id = format!("forge-mcp/{milestone_id}:{task_ref}");
        TaskHarness::claim_at(root.root(), &task_id, "contract", "contract-author").unwrap();
        let task = tasks::store_for_task(root.root(), &task_id)
            .unwrap()
            .inspect(&task_id)
            .unwrap();
        let mut contract = TaskHarness::default_evidence(&task);
        contract.proof = json!({"contract_proof":{"seam_test_ref":format!("src/{actual_dir}/hidden.rs::test_connected"),"red_exit_code":101}});
        let result = tasks::submit(
            root.root(),
            TaskHarness::pass_request(&task_id, "contract", &contract),
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
fn greenfield_seam_diagnostic_reports_missing_scope_files_and_scaffolding_guidance() {
    let root = prepare_indexed_boundary_workspace("pub fn route() -> usize { 1 }\n",
"",
"use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
"route",
None,
None,
"src/seam.rs::test_connected");
    std::fs::remove_file(root.root().join("src/api.rs")).unwrap();
    root.build_to(root.path(".forge/code-map.sqlite"));
    let task_id = "forge-mcp/m-current-boundary:routing";
    TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
    let mut store = tasks::store_for_task(root.root(), task_id).unwrap();
    let claimed = store.inspect(task_id).unwrap();
    let mut proof = TaskHarness::default_evidence(&claimed);
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
            root.build_to(root.path(".forge/code-map.sqlite"));
            observe_fixture_evidence(&claimed);
        }
        let result = store.submit(
            task_id,
            "contract",
            &proof,
            "pass",
            None,
            Some(&TaskHarness::session_for(&proof.worker_id)),
        );
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
