use super::*;

#[test]
fn executable_paths_expand_templates_and_disambiguate_leaf_aliases() {
    let profile: GateProfile = serde_yaml::from_str(
        r#"
gate_templates:
  cycle:
    gates:
      - id: build
      - id: review
gates:
  - id: first
    use: cycle
  - id: second
    use: cycle
"#,
    )
    .unwrap();
    let paths = profile
        .executable_gate_paths()
        .into_iter()
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "first.build",
            "first.review",
            "second.build",
            "second.review"
        ]
    );
    assert_eq!(profile.index_of("second.review"), Some(3));
    assert!(profile.by_stage("review").is_none());
    assert!(profile.by_stage("unknown.review").is_none());
}

#[test]
fn gate_guidance_renders_structured_criteria() {
    for (entry, expected) in [
        (GuidanceEntry::Text("plain".into()), "plain"),
        (
            GuidanceEntry::Structured {
                description: "empty".into(),
                criteria: vec![],
            },
            "empty",
        ),
        (
            GuidanceEntry::Structured {
                description: "review".into(),
                criteria: vec!["check scope".into(), "check invariants".into()],
            },
            "review\n- check scope\n- check invariants",
        ),
    ] {
        let mut profile = GateProfile::default();
        profile.guidance.insert("task.review".into(), entry.clone());
        let gate: GateDef = serde_yaml::from_str("id: review\nguidance: [task.review]\n").unwrap();
        assert_eq!(entry.rendered_text(), expected);
        assert_eq!(
            profile.resolve_gate_guidance(&gate, &BTreeMap::new()),
            vec![("task.review".into(), expected.into())],
        );
    }
}

#[test]
fn command_scope_patterns_are_retained_when_parsing_profiles() {
    let profile = parse_profile(
        r#"commands:
  check:
    command: cargo test
    scope_args_by_pattern:
      "crates/forge-tasks/**/*.rs": "--package forge-tasks --lib"
"#,
    )
    .unwrap();
    let CommandDef::Detailed {
        scope_args_by_pattern,
        ..
    } = &profile.commands["check"]
    else {
        panic!("expected a detailed command");
    };
    assert_eq!(
        scope_args_by_pattern["crates/forge-tasks/**/*.rs"],
        "--package forge-tasks --lib"
    );
    assert_eq!(
        profile
            .scope_args_for_command_path(
                &profile.commands["check"],
                "crates/forge-tasks/src/tasks/profile.rs",
            )
            .as_deref(),
        Some("--package forge-tasks --lib"),
    );
    assert_eq!(
        profile.scope_args_for_path("crates/forge-tasks/src/tasks/profile.rs"),
        None,
    );
}

#[test]
fn root_pattern_commands_include_every_matching_language_check() {
    let command = CommandDef::Pattern(PatternCommands {
        by_pattern: BTreeMap::from([
            ("src/*.rs".into(), "check-rust".into()),
            ("src/*.py".into(), "check-python".into()),
            ("docs/*.md".into(), "check-docs".into()),
        ]),
        description: None,
        timeout_seconds: None,
        output: None,
    });

    assert_eq!(
        command.commands_for_paths(&["src/lib.rs", "src/module.py"]),
        ["check-python", "check-rust"]
    );
    assert_eq!(command.commands_for_paths(&["src/lib.rs"]), ["check-rust"]);
}

#[test]
fn scope_selection_ranks_profile_and_command_patterns_together() {
    let profile: GateProfile = serde_yaml::from_str(
        r#"scope_args_by_pattern:
  "**/*.rs": "--lib"
  "tests/special/*.rs": "--test special"
  "tests/tie/*.rs": "--test global-tie"
commands:
  check:
    command: cargo test
    scope_args_by_pattern:
      "tests/*.rs": "--test {file_stem}"
      "tests/**/*.rs": "--test nested"
      "checks/*.rs": "--test {file_stem}"
      "tests/tie/*.rs": "--test local-tie"
      "tests/special/deep/*.rs": "--test deep"
"#,
    )
    .unwrap();
    let check_cmd = &profile.commands["check"];
    for (path, expected) in [
        ("src/lib.rs", Some("--lib")),
        ("tests/example.rs", Some("--test nested")),
        ("tests/special/case.rs", Some("--test special")),
        ("tests/special/deep/case.rs", Some("--test deep")),
        ("tests/tie/case.rs", Some("--test local-tie")),
        ("checks/example.rs", Some("--test example")),
        ("README.md", None),
    ] {
        assert_eq!(
            profile
                .scope_args_for_command_path(check_cmd, path)
                .as_deref(),
            expected,
            "{path}"
        );
    }
    assert_eq!(
        profile.scope_args_for_path("checks/example.rs").as_deref(),
        Some("--lib")
    );
    assert_eq!(profile.scope_args_for_path("README.md"), None);
    assert_eq!(
        profile.scope_args_for_path("src/lib.rs").as_deref(),
        Some("--lib")
    );
}

#[test]
fn command_override_does_not_leak_stale_scope_args() {
    let base = parse_profile(
        r#"commands:
  alpha:
    command: echo alpha
  beta:
    command: echo beta
    scope_args_by_pattern:
      "**/*.rs": "--beta-only"
"#,
    )
    .unwrap();
    assert_eq!(base.scope_args_for_path("src/lib.rs"), None);
    assert_eq!(
        base.scope_args_for_command_path(&base.commands["alpha"], "src/lib.rs"),
        None
    );
    assert_eq!(
        base.scope_args_for_command_path(&base.commands["beta"], "src/lib.rs")
            .as_deref(),
        Some("--beta-only")
    );

    let mut over = base.clone();
    let override_profile = parse_profile(
        r#"commands:
  beta:
    command: echo replacement
"#,
    )
    .unwrap();
    over.merge_override(override_profile);

    assert_eq!(over.scope_args_for_path("src/lib.rs"), None);
    assert_eq!(
        over.scope_args_for_command_path(&over.commands["beta"], "src/lib.rs"),
        None
    );
}

#[test]
fn save_alias_and_scalar_deserialization_works() {
    let yaml = r#"
gates:
  - id: gate1
    command: test_cmd
    save: commit
  - id: gate2
    command: test_cmd
    save: snapshot
  - id: gate3
    command: test_cmd
    save: logfile
  - id: gate4
    command: test_cmd
    save: none
  - id: gate5
    command: test_cmd
    save:
      pre: snapshot
      post: commit
"#;
    let profile = parse_profile(yaml).unwrap();
    assert_eq!(
        profile.gates[0].persistence.post,
        Some(PersistenceAction::Commit)
    );
    assert_eq!(
        profile.gates[1].persistence.post,
        Some(PersistenceAction::Snapshot)
    );
    assert_eq!(
        profile.gates[2].persistence.post,
        Some(PersistenceAction::Logfile)
    );
    assert_eq!(
        profile.gates[3].persistence.post,
        Some(PersistenceAction::None)
    );
    assert_eq!(
        profile.gates[4].persistence.pre,
        Some(PersistenceAction::Snapshot)
    );
    assert_eq!(
        profile.gates[4].persistence.post,
        Some(PersistenceAction::Commit)
    );
}

#[test]
fn loop_absorb_and_need_aliases_work() {
    let yaml = r#"
gates:
  - id: review
    role: reviewer
    loop:
      scope: build
      symbols: contract
  - id: deliver
    receipt: true
    need:
      - review
  - id: handoff
    command: test_cmd
    absorb: true
"#;
    let profile = parse_profile(yaml).unwrap();
    let loop_route = profile.gates[0].reject_routing.as_ref().unwrap();
    assert_eq!(loop_route.scope.as_deref(), Some("build"));
    assert_eq!(loop_route.symbols.as_deref(), Some("contract"));
    assert_eq!(profile.gates[1].review_sources, vec!["review"]);
    assert!(profile.gates[2].absorb_working_tree);
}

#[test]
fn commands_for_root_ignores_directory_names_and_discovers_untracked_nested_files() {
    let r = std::env::temp_dir().join(format!("forge-test-dispatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(&r).unwrap();
    let cmd = CommandDef::Pattern(PatternCommands {
        by_pattern: BTreeMap::from([
            ("**/*.rs".into(), "rust-check".into()),
            ("**/*.py".into(), "python-check".into()),
        ]),
        description: None,
        timeout_seconds: None,
        output: None,
    });
    assert_eq!(cmd.commands_for_root(&r), Vec::<&str>::new());
    std::fs::create_dir(r.join("directory.rs")).unwrap();
    assert_eq!(cmd.commands_for_root(&r), Vec::<&str>::new());
    std::fs::remove_dir(r.join("directory.rs")).unwrap();

    let _ = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&r)
        .status();
    std::fs::write(r.join("README.md"), "readme").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&r)
        .status();
    std::fs::create_dir(r.join("nested")).unwrap();
    std::fs::write(r.join("nested/source.py"), "print(1)").unwrap();
    assert_eq!(cmd.commands_for_root(&r), vec!["python-check"]);
    let _ = std::fs::remove_dir_all(&r);
}
