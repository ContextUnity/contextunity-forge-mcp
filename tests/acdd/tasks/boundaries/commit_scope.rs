use super::*;

#[test]
fn boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
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
        "gates:\n  - id: spec\n    role: contract_author\n    proof: contract\n    command: test\n    sha_snapshot: true\n  - id: implement\n    role: builder\n    proof: command\n    command: rust-check-and-test\n    sha_snapshot: true\n    expected_failure:\n      exit_codes: [1, 127]\n  - id: inspect\n    role: independent_reviewer\n    proof: review\n    reject_to: implement\n    independent_from: implement\n    contours: guidance.contours\n  - id: archive\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [inspect]\n    reject_to: implement\ncommands:\n  test:\n    command: \": --seam-ref src/routing.rs::test_connected && test -x .forge/runner_fixture && FORGE_FIXTURE_RED=1 .forge/runner_fixture\"\n  rust-check-and-test:\n    command: pytest\n    description: Preserve ordinary scoped paths for non-Cargo profiles.\n  python-quality: ruff\n",
    );
    let milestone = "---\nid: m-tiered-alias\ntitle: Proof-kind aliases\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve routing API boundaries\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs]\nscope_roots: [src/, tests/]\n```\n";
    root.write("docs/milestones/010-tiered-alias.md", milestone);
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-alias.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-alias:routing";
    TaskHarness::claim_at(root.root(), task_id, "spec", "spec-author").unwrap();
    let store = tasks::store_for_task(root.root(), task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let mut proof = TaskHarness::default_evidence(&task);
    proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "spec", &proof),
    )
    .unwrap();

    let claim = TaskHarness::claim_at(root.root(), task_id, "implement", "builder").unwrap();
    let scoped = &claim["workflow_guidance"]["scoped_commands"];
    assert!(
        scoped.as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["name"] == "rust-check-and-test"
                    && entry["test_path"] == "src/routing.rs"
                    && entry["command"] == "pytest src/routing.rs"
            }) && entries.iter().any(|entry| {
                entry["name"] == "python-quality"
                    && entry["test_path"] == "src/routing.rs"
                    && entry["command"] == "ruff src/routing.rs"
            })
        }),
        "scoped guidance must include every matching configured command with its ordinary test path: {scoped}"
    );

    root.write(
        "src/routing.rs",
        "pub fn route_task(value: usize) -> usize { value + 1 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n}\n",
    );
    root.write("tests/connected.rs", "use tiered_alias_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
    let task = store.inspect(task_id).unwrap();
    let mut proof = unobserved_evidence(&task);
    proof.proof = json!({"command_proof":{"command":"pytest src/routing.rs","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let error = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "implement", &proof),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert_eq!(store.inspect(task_id).unwrap().stage, "implement");
}
#[test]
fn commit_scope_validation_checks_exact_milestone_and_non_empty_scope() {
    use contextunity_forge_mcp::core::tasks::commit_files_within_scope;

    let milestone_ref = "docs/milestones/010-repack.md";
    let task_scope = ["src/lib.rs"];
    type CommitScopeCase<'a> = (&'a str, &'a [&'a str], Option<&'a [&'a str]>, bool);
    let cases: &[CommitScopeCase<'_>] = &[
        (
            "exact milestone file",
            &["docs/milestones/010-repack.md", "src/lib.rs"],
            None,
            true,
        ),
        (
            "milestone basename",
            &["010-repack.md", "src/lib.rs"],
            None,
            false,
        ),
        (
            "milestone path suffix",
            &["notdocs/milestones/010-repack.md", "src/lib.rs"],
            None,
            false,
        ),
        ("milestone extension", &["md", "src/lib.rs"], None, false),
        (
            "normalized leading dot",
            &["./docs/milestones/010-repack.md"],
            Some(&[]),
            true,
        ),
        (
            "normalized Windows separators",
            &["docs\\milestones\\010-repack.md"],
            Some(&[]),
            true,
        ),
        (
            "ACDD config requires explicit scope",
            &[".forge/acdd/profile.yaml"],
            None,
            false,
        ),
        (
            "framework config requires explicit scope",
            &[".forge/frameworks/rules.yaml"],
            None,
            false,
        ),
        (
            "explicit protected config scope",
            &[".forge/acdd/profile.yaml"],
            Some(&[".forge/acdd/profile.yaml"]),
            true,
        ),
        (
            "SQLite runtime artifact",
            &[".forge/tasks.sqlite"],
            Some(&[]),
            true,
        ),
        (
            "lock runtime artifact",
            &[".forge/tasks.lock"],
            Some(&[]),
            true,
        ),
        (
            "checkpoint runtime artifact",
            &[".forge/checkpoints.json"],
            Some(&[]),
            true,
        ),
        (
            "nested task SQLite runtime artifact",
            &[".forge/tasks/state.sqlite"],
            Some(&[]),
            true,
        ),
    ];

    for &(case, changed_files, scope_override, expected) in cases {
        let changed_files = changed_files
            .iter()
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        let scope = scope_override.unwrap_or(&task_scope);
        let scope = scope
            .iter()
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            commit_files_within_scope(&changed_files, &scope, Some(milestone_ref)),
            expected,
            "{case}"
        );
    }
}
#[test]
fn validate_commit_for_task_enforces_scope_and_ancestry_invariants() {
    use contextunity_forge_mcp::core::git::git_scoped_tree_changed;
    let h = TaskHarness::commit_history_fixture().unwrap();
    let scope = ["src/lib.rs".into()];
    assert!(h.validate(&h.delivery, &scope).is_ok());
    for obsolete in [&h.intermediate, &h.base] {
        let error = h.validate(obsolete, &scope).unwrap_err();
        assert!(
            error.to_string().contains("TASK_COMMIT_OBSOLETE"),
            "{error}"
        );
    }
    let error = h.validate(&h.delivery, &[]).unwrap_err();
    assert!(
        error.to_string().contains("TASK_COMMIT_OUTSIDE_SCOPE"),
        "{error}"
    );
    let doc_only = TaskHarness::commit_source(
        &h.root,
        "docs/milestones/010-test.md",
        "# Milestone\n",
        "Milestone doc only",
    )
    .unwrap();
    let error = h.validate(&doc_only, &[]).unwrap_err();
    assert!(
        error.to_string().contains("TASK_COMMIT_IGNORES_SCOPE"),
        "{error}"
    );
    let divergent = TaskHarness::commit_source(
        &h.root,
        "src/lib.rs",
        "pub fn base() {}\npub fn malicious_divergent() {}\n",
        "Divergent commit with different changes in scope",
    )
    .unwrap();
    let error = h.validate(&divergent, &scope).unwrap_err();
    assert!(
        error.to_string().contains("TASK_COMMIT_TREE_MISMATCH"),
        "{error}"
    );
    assert!(
        git_scoped_tree_changed(h.root.root(), &h.delivery, &divergent, &[]).unwrap(),
        "empty scope must detect full tree difference"
    );
    assert!(
        !git_scoped_tree_changed(h.root.root(), &h.delivery, &h.delivery, &[]).unwrap(),
        "empty scope must return false for identical commits"
    );
}
