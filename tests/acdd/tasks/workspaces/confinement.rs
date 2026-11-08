use super::*;
use crate::acdd::tasks::harness::TaskHarness;

#[test]
fn scope_extension_rejects_paths_owned_by_sibling_tasks_and_admits_unowned_tests_and_src() {
    let mut memory = TaskHarness::memory();
    let memory_id = memory.id.clone();
    let memory_root = memory.root().to_path_buf();
    let before_extension = memory.store.inspect(&memory_id).unwrap();
    let before_paths = TaskHarness::persisted_scope_path_count(&memory.store, &memory_id).unwrap();
    let extension_error = memory
        .store
        .extend_scope(
            &memory_id,
            &[".forge/acdd/profile.yaml".into()],
            &memory_root,
        )
        .unwrap_err();
    assert!(extension_error.to_string().contains("TASK_SCOPE_PROTECTED"));
    let after_extension = memory.store.inspect(&memory_id).unwrap();
    assert_eq!(after_extension.spec.scope, before_extension.spec.scope);
    assert_eq!(after_extension.status, before_extension.status);
    let after_paths = TaskHarness::persisted_scope_path_count(&memory.store, &memory_id).unwrap();
    assert_eq!(
        after_paths, before_paths,
        "rejected extension added scope rows"
    );

    let root = acdd_workspace(DEFAULT_SEAM, "");
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
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-conflict.md", root.root())
        .unwrap();

    let task_b_id = "forge-mcp/m-conflict:task_b";
    let task_a_id = "forge-mcp/m-conflict:task_a";

    TaskHarness::claim_at(root.root(), task_b_id, "contract", "worker_b").unwrap();

    let assert_scope_conflict = |requester: &str, path: &str, owner: &str, shape: &str| {
        let err = tasks::manage(
            root.root(),
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
            root.root(),
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
    TaskHarness::claim_at(root.root(), task_a_id, "contract", "worker_a").unwrap();

    store.reset(task_a_id).unwrap();

    let dynamic_count = TaskHarness::unfrozen_scope_path_count(&store, task_b_id).unwrap();
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

    let claim_root = acdd_workspace(DEFAULT_SEAM, "");
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
    let mut claim_store = TasksStore::open(&claim_root.root().join(".forge/tasks.sqlite")).unwrap();
    claim_store
        .sync(
            &claim_milestone,
            "docs/010-claim-conflict.md",
            claim_root.root(),
        )
        .unwrap();
    let owner_id = "forge-mcp/m-claim-conflict:owner";
    TaskHarness::claim_at(claim_root.root(), owner_id, "contract", "owner").unwrap();
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
            claim_root.root(),
        )
        .unwrap();
    let claim_error = tasks::claim(
        claim_root.root(),
        tasks::Claim {
            task_id: "forge-mcp/m-claim-conflict:successor".into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            session_fingerprint: Some(TaskHarness::session_for("successor")),
            worktree: claim_root.root().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(claim_error.contains("TASK_NOT_READY"), "{claim_error}");
    assert!(claim_error.contains(owner_id), "{claim_error}");

    let release_root = acdd_workspace(DEFAULT_SEAM, "");
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
        delivery_gate: None,
        candidate_gate: None,
        commit: None,
        contract_revision: 1,
        passed_at: "2026-10-10T00:00:00Z".into(),
        evidence: json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}}),
        review: passing_review_proof(),
        decision: "pass".into(),
        rollup: None,
    });
    let mut release_store =
        TasksStore::open(&release_root.root().join(".forge/tasks.sqlite")).unwrap();
    release_store
        .sync(
            &release_milestone,
            "docs/010-scope-release.md",
            release_root.root(),
        )
        .unwrap();
    let completed_owner_id = "forge-mcp/m-scope-release:completed_owner";
    let retained_scope_count =
        TaskHarness::persisted_scope_path_count(&release_store, completed_owner_id).unwrap();
    assert_eq!(retained_scope_count, 1);
    let successor_id = "forge-mcp/m-scope-release:successor";
    let claimed =
        TaskHarness::claim_at(release_root.root(), successor_id, "contract", "successor").unwrap();
    assert_eq!(claimed["status"], "in_progress");
    let extended = tasks::manage(
        release_root.root(),
        serde_json::from_value(json!({
            "action": "extend_scope",
            "task_id": successor_id,
            "paths": ["src/released/extra.rs"]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(extended["status"], "in_progress");

    for tracked in [false, true] {
        let root = acdd_workspace(DEFAULT_SEAM, "");
        root.write("src/lib.rs", "pub fn protected() {}\n");
        root.write(".git/info/exclude", "");
        root.write(".forge/acdd/profile.yaml", "\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let contract = "---\nid: m-protected\ntitle: Protected scope\ndoc_type: contract\nstatus: active\n---\n# Protected scope\n```yaml\ntask_ref: protected\ntarget: Reject dirty protected configuration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
        root.write("docs/010-protected.md", contract);
        let milestone = Milestone::parse(contract, "forge-mcp").unwrap();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
        store
            .sync(&milestone, "docs/010-protected.md", root.root())
            .unwrap();
        if tracked {
            let added = TaskHarness::stage_changes(root.root(), &[".forge/acdd/profile.yaml"]);
            assert!(added.status.success());
            let committed = TaskHarness::commit_fixture_with_test_identity(
                root.root(),
                "track protected profile",
            );
            assert!(
                committed.status.success(),
                "{}",
                String::from_utf8_lossy(&committed.stderr)
            );
            root.write(".forge/acdd/profile.yaml", "# changed after commit\n");
        }

        let before = store.inspect(&task_id).unwrap();
        let claims_before = TaskHarness::claim_count(&store, &task_id).unwrap();
        let error =
            TaskHarness::claim_at(root.root(), &task_id, "contract", "protected-scope-check")
                .unwrap_err();
        assert!(
            error.to_string().contains("TASK_SCOPE_VIOLATION"),
            "tracked={tracked}: {error:#}"
        );
        let after = store.inspect(&task_id).unwrap();
        assert_eq!(after.status, before.status);
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.claim_revision, before.claim_revision);
        let claims_after = TaskHarness::claim_count(&store, &task_id).unwrap();
        assert_eq!(
            claims_after, claims_before,
            "tracked={tracked} claim mutated DB"
        );
    }

    let admitted_root = acdd_workspace(DEFAULT_SEAM, "");
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
        TasksStore::open(&admitted_root.root().join(".forge/tasks.sqlite")).unwrap();
    admitted_store
        .sync(
            &admitted_milestone,
            "docs/010-protected-admitted.md",
            admitted_root.root(),
        )
        .unwrap();
    let admitted = TaskHarness::claim_at(
        admitted_root.root(),
        &admitted_id,
        "contract",
        "admitted-protected-scope",
    )
    .unwrap();
    assert_eq!(admitted["status"], "in_progress");
    assert_eq!(
        admitted_store.inspect(&admitted_id).unwrap().initial_scope,
        vec![".forge/acdd/profile.yaml"]
    );
}

#[test]
fn universal_scope_roots_extend_scope_and_boundary_validation() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
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
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-universal.md", root.root())
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
            root.root(),
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
            root.root(),
        )
        .unwrap_err();
    assert!(
        err_auth.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID, got: {err_auth}"
    );

    let err_src = store
        .extend_scope(catalogue_id, &["src/anything.rs".into()], root.root())
        .unwrap_err();
    assert!(
        err_src.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for src outside scope_roots, got: {err_src}"
    );

    // 3. Fallback when scope_roots is omitted: auto-derives base from initial scope
    let ext_payment = store
        .extend_scope(
            payment_id,
            &["services/payment/utils.py".into()],
            root.root(),
        )
        .unwrap();
    assert!(ext_payment
        .spec
        .scope
        .contains(&"services/payment/utils.py".into()));

    let err_billing = store
        .extend_scope(
            payment_id,
            &["services/billing/invoice.py".into()],
            root.root(),
        )
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
        .sync(&root_milestone, "docs/010-root.md", root.root())
        .unwrap();
    let installer_id = "forge-mcp/m-root:installer_task";

    let err_root_cargo = store
        .extend_scope(installer_id, &["Cargo.toml".into()], root.root())
        .unwrap_err();
    assert!(
        err_root_cargo.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for Cargo.toml from root-level install.sh, got: {err_root_cargo}"
    );

    let err_root_src = store
        .extend_scope(installer_id, &["src/lib.rs".into()], root.root())
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
#[cfg(unix)]
fn task_profile_resolution_rejects_file_and_parent_symlink_escapes() {
    use sha2::Digest;
    use std::os::unix::fs::symlink;

    let profile = "roles:\n  independent_reviewer:\n    model: outside-workspace\n";
    let external = TaskHarness::workspace_with_file("profiles/team.yaml", profile);
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
        let root = TaskHarness::runner_workspace(DEFAULT_SEAM, "");
        root.write("src/lib.rs", "pub fn profile() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let link = root.root().join(if name == "file" {
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
            root.root(),
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
    let external_default =
        TaskHarness::workspace_with_file("profiles/profile.yaml", external_profile);
    let mut automatic_default_escapes = Vec::new();

    for (name, link_kind, expected_model) in [
        ("default_file", "file", None),
        ("default_parent", "parent", None),
        ("default_local", "local", Some("in-root-profile")),
    ] {
        let root = TaskHarness::runner_workspace(DEFAULT_SEAM, "");
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

        std::fs::remove_dir_all(root.root().join(".forge/acdd")).unwrap();
        match link_kind {
            "file" => {
                std::fs::create_dir_all(root.root().join(".forge/acdd")).unwrap();
                symlink(
                    external_default.root().join("profiles/profile.yaml"),
                    root.root().join(".forge/acdd/profile.yaml"),
                )
                .unwrap();
            }
            "parent" => {
                std::fs::create_dir_all(root.root().join(".forge")).unwrap();
                symlink(
                    external_default.root().join("profiles"),
                    root.root().join(".forge/acdd"),
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

        let sync = TaskHarness::run_cli_for_root(
            root.root(),
            &[
                "--format",
                "json",
                "task",
                "sync",
                "docs/milestones/010-profile.md",
            ],
        );

        if let Some(expected_model) = expected_model {
            assert!(
                sync.status.success(),
                "ordinary in-root default profile should load: {}",
                String::from_utf8_lossy(&sync.stderr)
            );
            let synced: Value = serde_json::from_slice(&sync.stdout).unwrap();
            let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
            let inspect = TaskHarness::run_cli_for_root(
                root.root(),
                &["--format", "json", "task", "inspect", task_id],
            );
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
            let inspect = TaskHarness::run_cli_for_root(
                root.root(),
                &["--format", "json", "task", "inspect", task_id],
            );
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
