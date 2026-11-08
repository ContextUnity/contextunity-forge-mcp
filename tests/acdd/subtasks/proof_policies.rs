use super::*;

#[test]
fn direct_proof_and_deferred_final_test_accept_zero_exit_code_and_seam_test_first_rejects_it() {
    let spec = "---\nid: m-policy\ntitle: Policy Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/]\n```\n```yaml\ntask_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/]\n```\n```yaml\ntask_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn policy() {}\n");
    root.write("src/direct.rs", "pub fn direct() {}\n");
    root.write("src/deferred.rs", "pub fn deferred() {}\n");
    root.write("src/seam.rs", "pub fn seam() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = spec
        .replace(
            "task_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/]",
            "task_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/direct.rs]",
        )
        .replace(
            "task_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/]",
            "task_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/deferred.rs]",
        )
        .replace(
            "task_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/]",
            "task_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/seam.rs]",
        );
    root.write("docs/010-policy.md", &spec);
    let milestone = Milestone::parse(&spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-policy.md", root.root())
        .unwrap();

    let direct_id = "forge-mcp/m-policy:direct";
    store = tasks::store_for_task(root.root(), direct_id).unwrap();
    root.write(".forge/runner_fixture_green", "green\n");

    for (task_ref, proof_policy, zero_exit_allowed) in [
        ("direct", "direct-proof", true),
        ("deferred", "deferred-final-test", true),
        ("seam", "seam-test-first", false),
    ] {
        if !zero_exit_allowed {
            std::fs::remove_file(root.root().join(".forge/runner_fixture_green")).unwrap();
        }
        let task_id = format!("forge-mcp/m-policy:{task_ref}");
        store
            .claim(
                &task_id,
                "contract",
                "builder",
                root.root().to_str().unwrap(),
                Some("test-session-builder"),
            )
            .unwrap();
        let task = store.inspect(&task_id).unwrap();
        assert_eq!(task.spec.proof_policy, proof_policy);
        let mut submitted_evidence = evidence(&task);
        submitted_evidence.proof = json!({
            "contract_proof": {
                "seam_test_ref": "tests/acdd/tasks.rs",
                "red_exit_code": 0
            }
        });
        let submission = store.submit(
            &task_id,
            "contract",
            &submitted_evidence,
            "pass",
            None,
            Some("test-session-builder"),
        );
        if zero_exit_allowed {
            assert!(
                submission.is_ok(),
                "{proof_policy} policy must accept exit code 0"
            );
            continue;
        }

        let error = submission.unwrap_err();
        assert!(error
            .to_string()
            .contains("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required"));
        submitted_evidence.proof["contract_proof"]["red_exit_code"] = json!(101);
        assert!(
            store
                .submit(
                    &task_id,
                    "contract",
                    &submitted_evidence,
                    "pass",
                    None,
                    Some("test-session-builder"),
                )
                .is_ok(),
            "{proof_policy} policy must accept nonzero exit code"
        );
    }
}
