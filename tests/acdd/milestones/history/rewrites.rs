use super::*;

fn history_baseline(config: &str) -> DeliveredBaseline {
    let source = MilestoneFixtureBuilder::new("m-history")
        .title("Content reviewed handoff")
        .task("first", &["src/shared.rs"])
        .task_target("Update first function")
        .task_with_deps("second", &["src/shared.rs"], &["first"])
        .task_target("Update second function")
        .build();
    let initial = (0..24)
        .map(|i| format!("pub fn f{i}() -> u8 {{ 0 }}\n"))
        .collect::<String>();
    let mut reviewed = initial.clone();
    let tasks = (0..2)
        .map(|index| {
            reviewed = reviewed.replace(
                &format!("f{index}() -> u8 {{ 0 }}"),
                &format!("f{index}() -> u8 {{ {} }}", index + 1),
            );
            DeliveredTask {
                task_ref: if index == 0 { "first" } else { "second" },
                updates: vec![("src/shared.rs", reviewed.clone())],
                completion: DeliveredCompletion::Builder(if index == 0 {
                    "builder-first"
                } else {
                    "builder-second"
                }),
            }
        })
        .collect();
    delivered(DeliveredProfile {
        milestone_id: "m-history",
        active_path: "docs/milestones/010-history.md",
        source: &source,
        config,
        files: vec![
            (".gitignore", ".forge/\n".into()),
            ("src/shared.rs", initial),
        ],
        seed_paths: &[".gitignore", "forge-mcp.yaml", "docs", "src"],
        commit_message: "admit history fixture",
        disable_git_maintenance: true,
        tasks,
        evidence: DeliveredEvidence::Observed {
            reviewer: "integration-reviewer",
            summary: "Review combined history fixture",
        },
        capture_sqlite: false,
    })
}

fn rewrite_baseline() -> DeliveredBaseline {
    history_baseline("roots: [src]\ntasks_db: .forge/tasks.sqlite\n")
}

#[test]
fn handoff_preserves_proofs_across_reviewed_history_rewrites() {
    check_history_rewrites(
        rewrite_baseline(),
        &["orphan", "amend", "cherry-pick", "merge"],
    );
}

#[test]
fn handoff_preserves_proofs_across_reviewed_history_rebase_squash_conflict_and_gc() {
    check_history_rewrites(rewrite_baseline(), &["rebase", "squash", "conflict", "gc"]);
}

fn check_history_rewrites(baseline: DeliveredBaseline, rewrites: &[&str]) {
    let DeliveredBaseline {
        root,
        base,
        branch,
        deliveries,
        before,
        evidence,
        snapshot,
        ..
    } = baseline;
    let initial = (0..24)
        .map(|i| format!("pub fn f{i}() -> u8 {{ 0 }}\n"))
        .collect::<String>();
    let reviewed = initial
        .replace("f0() -> u8 { 0 }", "f0() -> u8 { 1 }")
        .replace("f1() -> u8 { 0 }", "f1() -> u8 { 2 }");

    assert_eq!(
        evidence.binding.builders,
        vec!["builder-first", "builder-second"]
    );
    let mut needs_restore = false;
    for &scenario in rewrites {
        if needs_restore {
            snapshot.restore(&root);
        }
        needs_restore = true;
        let mut evidence = evidence.clone();
        let active = "docs/milestones/010-history.md";
        let archived = "docs/milestones/archive/010-history.md";

        match scenario {
            "orphan" => {
                root.create_orphan_branch("rewritten");
                root.commit_staged("same content without original ancestry");
            }
            "amend" => {
                root.amend_message("amended navigation identity");
            }
            "cherry-pick" => {
                root.create_branch_at("picked", &base);
                MilestoneHarness::git_at(
                    root.root(),
                    &["cherry-pick", "--no-commit", &deliveries[0], &deliveries[1]],
                )
                .unwrap();
                root.commit_staged("picked reviewed content");
            }
            "squash" => {
                MilestoneHarness::git_at(root.root(), &["reset", "--soft", &base]).unwrap();
                root.commit_staged("two tasks in one commit");
            }
            "merge" | "rebase" | "conflict" => {
                root.create_branch_at("upstream", &base);
                let upstream = if scenario == "conflict" {
                    initial.replacen("{ 0 }", "{ 3 }", 1)
                } else {
                    initial.replace("f23() -> u8 { 0 }", "f23() -> u8 { 3 }")
                };
                root.write("src/shared.rs", &upstream);
                root.commit_tracked("upstream optimization in the same file");
                let target = root.head();
                root.select_revision(&branch);
                if scenario == "rebase" {
                    MilestoneHarness::git_at(root.root(), &["rebase", "upstream"]).unwrap();
                } else if scenario == "conflict" {
                    let output = root.merge_branch("upstream", "integrate upstream");
                    assert!(!output.status.success());
                    root.write(
                        "src/shared.rs",
                        &reviewed.replace("f0() -> u8 { 1 }", "f0() -> u8 { 4 }"),
                    );
                    root.stage_paths(&["src/shared.rs"]);
                    MilestoneHarness::git_at(root.root(), &["commit", "--no-edit"]).unwrap();
                } else {
                    {
                        let merged = root.merge_branch("upstream", "integrate upstream");
                        assert!(
                            merged.status.success(),
                            "{}",
                            String::from_utf8_lossy(&merged.stderr)
                        );
                    };
                }
                let stale = root.handoff_at("m-history", None, &evidence);
                assert!(stale.is_err(), "{scenario}: stale evidence should fail");
                assert!(
                    stale
                        .unwrap_err()
                        .to_string()
                        .contains("HANDOFF_EVIDENCE_STALE"),
                    "{scenario}"
                );
                assert!(root.path(active).exists());
                assert!(!root.path(archived).exists());
                let (fresh, observed) =
                    MilestoneHarness::reverify(&root, "m-history", &target, &evidence);
                assert_eq!(observed["verification"]["exit_code"], 0);
                assert_eq!(observed["verification"]["tests_failed"], 0);
                evidence = fresh;
            }
            "gc" => {
                let tree = root.tree_at("HEAD");
                let obsolete = root.create_navigation_commit(&tree, "temporary navigation object");
                let document = root.document(active);
                let store = root.open_task_store().unwrap();
                let milestone = Milestone::parse(&document, "forge-mcp").unwrap();
                let mut receipt = store
                    .inspect(&milestone.task_id(&milestone.tasks[0]))
                    .unwrap()
                    .receipt
                    .unwrap();
                receipt.commit = Some(obsolete.clone());
                root.write(
                    active,
                    &milestone_engine::render_task_receipt(&document, "first", &receipt, &[])
                        .unwrap(),
                );
                drop(store);
                root.stage_paths(&[active]);
                root.commit_staged("navigation pointer update");
                MilestoneHarness::git_at(
                    root.root(),
                    &["reflog", "expire", "--expire=now", "--all"],
                )
                .unwrap();
                MilestoneHarness::git_at(root.root(), &["gc", "--prune=now"]).unwrap();

                let exists = MilestoneHarness::git_output_at(
                    root.root(),
                    ["cat-file", "-e", &format!("{obsolete}^{{commit}}")],
                )
                .status
                .success();
                assert!(
                    !exists,
                    "unreferenced navigation object survived pruning: root={} object={obsolete} packs={}",
                    root.root().display(),
                    MilestoneHarness::git_text_at(root.root(), &["count-objects", "-v"])
                );
            }
            _ => unreachable!("all history rewrites are listed above"),
        }
        let handoff_res = root.handoff_at("m-history", None, &evidence);
        assert!(
            handoff_res.is_ok(),
            "{scenario}: handoff failed: {:?}",
            handoff_res.err()
        );
        let output = handoff_res.unwrap();
        assert_eq!(
            output["handoff"]["integration_evidence"],
            serde_json::to_value(&evidence).unwrap()
        );
        let archived_text = root.document(archived);
        let archived_milestone = Milestone::parse(&archived_text, "forge-mcp").unwrap();
        let store = root.open_task_store().unwrap();
        assert!(store.list(None, "all", None).unwrap().is_empty());
        for (index, spec) in archived_milestone.tasks.iter().enumerate() {
            let accepted: contextunity_forge_mcp::core::tasks::Receipt =
                serde_json::from_value(before[index]["receipt"].clone()).unwrap();
            assert!(accepted.matches_proofs(spec.receipt.as_ref().unwrap()));
        }
        assert!(!root.path(active).exists());
        assert!(root.path(archived).exists());
        let refs = root.refs_with_prefix("refs/forge/evidence/");
        let meta = MilestoneHarness::frontmatter(&archived_text);
        let retained: std::collections::BTreeSet<_> = refs.lines().collect();
        for sha in meta["handoff"]["proof_shas"].as_sequence().unwrap() {
            assert!(!retained
                .contains(format!("refs/forge/evidence/{}", sha.as_str().unwrap()).as_str()));
        }
        assert!(root.refs_with_prefix("refs/forge/snapshots/").is_empty());
    }
}

#[test]
fn handoff_preserves_proofs_across_commit_tracking_policies() {
    let policies = [
        ("off", "commit_tracking: off\n"),
        ("milestone-only", "commit_tracking: milestone_only\n"),
        ("legacy-off", "record_task_commits: false\n"),
    ];
    let DeliveredBaseline {
        root,
        base,
        deliveries,
        before,
        evidence,
        snapshot,
        ..
    } = history_baseline("roots: [src]\ntasks_db: .forge/tasks.sqlite\n");

    assert_eq!(
        evidence.binding.builders,
        vec!["builder-first", "builder-second"]
    );
    for (index, (scenario, setting)) in policies.into_iter().enumerate() {
        if index > 0 {
            snapshot.restore(&root);
        }
        root.reset_history_to(&base);
        root.write(
            "forge-mcp.yaml",
            &format!("roots: [src]\ntasks_db: .forge/tasks.sqlite\n{setting}"),
        );
        root.stage_paths(&["forge-mcp.yaml"]);
        root.commit_staged("select handoff recording policy");
        let policy_base = root.head();
        root.cherry_pick_commit(&deliveries[0]);
        root.cherry_pick_commit(&deliveries[1]);
        let (evidence, observed) =
            MilestoneHarness::reverify(&root, "m-history", &policy_base, &evidence);
        assert_eq!(observed["verification"]["exit_code"], 0);
        assert_eq!(observed["verification"]["tests_failed"], 0);

        let handoff_res = root.handoff_at("m-history", None, &evidence);
        assert!(
            handoff_res.is_ok(),
            "{scenario}: handoff failed: {:?}",
            handoff_res.err()
        );
        let output = handoff_res.unwrap();
        assert_eq!(
            output["handoff"]["integration_evidence"],
            serde_json::to_value(&evidence).unwrap()
        );
        let archived_text = root.document("docs/milestones/archive/010-history.md");
        let archived_milestone = Milestone::parse(&archived_text, "forge-mcp").unwrap();
        let store = root.open_task_store().unwrap();
        assert!(store.list(None, "all", None).unwrap().is_empty());
        for (index, spec) in archived_milestone.tasks.iter().enumerate() {
            let accepted: contextunity_forge_mcp::core::tasks::Receipt =
                serde_json::from_value(before[index]["receipt"].clone()).unwrap();
            assert!(accepted.matches_proofs(spec.receipt.as_ref().unwrap()));
            assert!(spec.receipt.as_ref().unwrap().commit.is_none());
        }
        assert!(!root.path("docs/milestones/010-history.md").exists());
        assert!(root.path("docs/milestones/archive/010-history.md").exists());
        let refs = root.refs_with_prefix("refs/forge/evidence/");
        let meta = MilestoneHarness::frontmatter(&archived_text);
        let retained: std::collections::BTreeSet<_> = refs.lines().collect();
        for sha in meta["handoff"]["proof_shas"].as_sequence().unwrap() {
            assert!(!retained
                .contains(format!("refs/forge/evidence/{}", sha.as_str().unwrap()).as_str()));
        }
        assert!(root.refs_with_prefix("refs/forge/snapshots/").is_empty());
    }
}
