use super::*;

#[test]
fn milestone_cli_smoke_parses_arguments_and_reports_exit_codes() {
    let root = MilestoneHarness::configured_workspace();
    let run = |args: &[&str]| root.cli(&[&["milestone"], args].concat(), None);
    let listed = run(&["list", "--archive", "--limit", "1"]);
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let value: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(value["archive_displayed"], 0);

    let missing_id = run(&["show"]);
    assert_eq!(missing_id.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_id.stderr).contains("<ID>"));
    root.write(".forge/cli-handoff-evidence.json", "{}");
    let malformed = run(&[
        "handoff",
        "m-cli-missing",
        "--evidence-file",
        root.path(".forge/cli-handoff-evidence.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(malformed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("missing field `binding`"));
}

#[test]
fn milestone_cli_init_reads_outcome_from_stdin() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    let initialized = root.cli(
        &[
            "milestone",
            "init",
            "--num",
            "001",
            "--slug",
            "cli-input",
            "--title",
            "CLI input",
        ],
        Some("Outcome supplied on stdin.\n"),
    );
    assert!(initialized.status.success());
    let created = root.document("docs/milestones/001-cli-input.md");
    assert!(created.contains("Outcome supplied on stdin."));
}

#[test]
fn milestone_in_process_cli_reports_application_errors_and_required_evidence() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    let commit = root.head();
    let application_cases = [
        (
            vec![
                "handoff-context",
                "m-cli-missing",
                "--target-base",
                commit.as_str(),
            ],
            "milestone selector must resolve to exactly one active document: m-cli-missing",
        ),
        (
            vec!["verify", "m-cli-missing", "--target-base", commit.as_str()],
            "milestone selector must resolve to exactly one active document: m-cli-missing",
        ),
    ];
    for (args, diagnostic) in application_cases {
        let error = root
            .run_cli(&[&["milestone"], args.as_slice()].concat())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains(diagnostic),
            "{args:?}: {error:#}"
        );
    }

    let missing_evidence = root
        .parse_cli(&[
            "milestone",
            "handoff",
            "m-cli-missing",
            "--commit",
            &commit,
            "--evidence-file",
        ])
        .unwrap_err();
    assert_eq!(missing_evidence.exit_code(), 2);
    assert!(missing_evidence.to_string().contains("--evidence-file"));
}

#[test]
fn task_claim_in_process_cli_parses_arguments_and_returns_guidance() {
    let root = MilestoneHarness::configured_workspace();
    root.write("src/lib.rs", "pub fn task() {}\n");
    let source = MilestoneFixtureBuilder::new("m-cli-claim")
        .title("CLI claim")
        .status("planned")
        .task("begin", &["src/"])
        .task_target("Begin work")
        .build();
    root.write("docs/milestones/010-cli-claim.md", &source);
    let (milestone, store) = root.sync_document("docs/milestones/010-cli-claim.md", &source);
    let id = milestone.task_id(&milestone.tasks[0]);
    drop(store);

    let claimed = root.run_cli(&[
        "task",
        "claim",
        &id,
        "--stage",
        "contract",
        "--worker",
        "first",
        "--worktree",
        root.root().to_str().unwrap(),
    ]);
    assert!(claimed.is_ok(), "{claimed:#?}");
    let claim = claimed.unwrap();
    assert!(serde_json::to_string(&claim["workflow_guidance"])
        .unwrap()
        .contains("PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile"));
    let missing_id = root.parse_cli(&["task", "claim"]).unwrap_err();
    assert_eq!(missing_id.exit_code(), 2);
}
