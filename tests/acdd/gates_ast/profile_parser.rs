use super::*;

#[test]
fn profile_parser_accepts_polymorphic_drivers_and_catalogs() {
    let cases = [
        (
            "command gate driver",
            r#"
commands:
  check_task: "check {scope_target}"
gates:
  - id: verify
    command: check_task
    proof: command
"#,
        ),
        (
            "by_pattern command catalog",
            r#"
commands:
  check_task:
    by_pattern:
      "**/*.py": "typecheck {scope_target} && test {scope_target}"
      "**/*.rs": "check {scope_target}"
"#,
        ),
        (
            "guidance catalog",
            r#"
guidance:
  task.contract: "Author red test."
"#,
        ),
    ];

    for (case, yaml) in cases {
        let parsed = parse_profile(yaml);
        assert!(
            parsed.is_ok(),
            "{case} should deserialize through the production profile parser: {parsed:?}"
        );
    }

    let v2 = validate_profile_document(include_str!(
        "../../../docs/future/references/acdd_profile.v2.yaml.example"
    ))
    .expect("the v2 profile example should satisfy the production validators");
    assert_eq!(v2.version, Some(2));
    assert_eq!(
        v2.commands["check_task"].command_for_path("src/lib.rs"),
        Some(
            "cargo clippy --lib --test {scope_target} && cargo test --test {scope_target} -- --nocapture"
        )
    );
    assert_eq!(
        v2.commands["check_task"].command_for_path("web/pages/home.tsx"),
        Some("npm run typecheck && npm test -- {scope_target}")
    );
    assert_eq!(
        v2.commands["check_task"].command_for_path("README.md"),
        None
    );
    assert_eq!(v2.gates[0].driver(), Some(GateDriver::Agent));
    let valid_bindings = [
        (
            "fully qualified override",
            [("hitl.pro".to_owned(), "hitl.paranoic".to_owned())].into(),
        ),
        (
            "short target and value",
            [("hitl".to_owned(), "pro".to_owned())].into(),
        ),
        (
            "short target with short value paranoic",
            [("hitl".to_owned(), "paranoic".to_owned())].into(),
        ),
        (
            "short target with full value",
            [("hitl".to_owned(), "hitl.paranoic".to_owned())].into(),
        ),
        (
            "full target with short value",
            [("hitl.pro".to_owned(), "paranoic".to_owned())].into(),
        ),
        ("empty overrides", Default::default()),
    ];
    for (case, bindings) in valid_bindings {
        let issues = v2
            .validate_guidance_bindings(&bindings)
            .expect("guidance binding validation should succeed");
        assert!(issues.is_empty(), "{case}: {issues:?}");
    }

    let invalid_bindings = [
        (
            "misspelled replacement",
            [("hitl.pro".to_owned(), "hitl.misspelled".to_owned())].into(),
        ),
        (
            "misspelled short replacement",
            [("hitl".to_owned(), "proo".to_owned())].into(),
        ),
        (
            "misspelled target",
            [("hitl.proo".to_owned(), "hitl.paranoic".to_owned())].into(),
        ),
        (
            "unknown target",
            [("unrelated_target".to_owned(), "hitl.paranoic".to_owned())].into(),
        ),
        (
            "unknown entry",
            [("missing".to_owned(), "missing_entry".to_owned())].into(),
        ),
    ];
    for (case, bindings) in invalid_bindings {
        assert!(
            v2.validate_guidance_bindings(&bindings).is_err(),
            "{case} should be rejected"
        );
    }
    let contract_gate = &v2.gates[1].tasks.as_ref().expect("tasks container").gates[0];
    for bindings in [
        [("hitl.pro".to_owned(), "hitl.paranoic".to_owned())].into(),
        [("hitl".to_owned(), "paranoic".to_owned())].into(),
    ] {
        let resolved = v2.resolve_gate_guidance(contract_gate, &bindings);
        assert!(
            resolved
                .iter()
                .any(|(key, value)| key == "hitl.pro" && value.contains("PARANOIC")),
            "resolved guidance should contain the case-insensitive override"
        );
    }
    let standard_contours = v2.contour_set(None).expect("standard contour set exists");
    assert_eq!(
        standard_contours["paths"].description,
        v2.guidance["contour.paths"].text()
    );
    assert_eq!(
        standard_contours["paths"].criteria,
        v2.guidance["contour.paths"].criteria()
    );
    assert!(validate_profile_document(
        "version: 2\nroles:\n  worker: {}\ngates:\n  - id: symbols\n    role: worker\n    proof: none\n    symbols: [not-a-symbol-coordinate]\n"
    )
    .is_err());

    let alias_conditions = r#"
version: 2
roles:
  worker: {}
gates:
  - id: docs
    when: docs
    role: worker
    proof: none
  - id: database
    when: db
    role: worker
    proof: none
  - id: handoff
    role: worker
    proof: delivery
"#;
    assert!(validate_profile_document(alias_conditions).is_ok());

    let invalid_profiles = [
        (
            "duplicate hierarchical gate ids",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none}\n  - {id: build, role: worker, proof: none}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "strictly prior gate references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, reject_to: future}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "review sources must point to a prior review gate",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none}\n  - {id: review, role: worker, proof: review, review_sources: [build]}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "reject routing targets must resolve in the profile",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: review, role: worker, proof: review, reject_routing: {symbols: missing}}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "registered command references",
            "version: 2\nroles: {worker: {}}\ncommands: {check: \"echo ok\"}\ngates:\n  - {id: verify, command: missing, proof: command}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "guidance slot and catalog references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, guidance: [missing]}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "contour references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: review, role: worker, proof: review, contours: missing}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "typed gate conditions",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, when: rust}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "terminal hierarchical delivery",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: handoff, role: worker, proof: delivery}\n  - {id: after, role: worker, proof: none}\n",
        ),
        (
            "contour definitions require usable criteria",
            "version: 2\nroles: {worker: {}}\nguidance:\n  contour.paths: {description: \"\", criteria: []}\ngates:\n  - {id: review, role: worker, proof: review, contours: guidance.contours}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "legacy contours catalog is rejected",
            "version: 2\nroles: {worker: {}}\ncontours:\n  standard:\n    paths: {description: \"desc\", criteria: [\"crit\"]}\ngates:\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
    ];
    for (case, yaml) in invalid_profiles {
        assert!(
            validate_profile_document(yaml).is_err(),
            "hierarchical validation must reject {case}"
        );
    }

    let milestone = Milestone::parse(
        r#"---
id: m-linked
required_skills: [contextunity-forge]
repositories:
  - name: forge-mcp
  - name: tools
    path: tools
linked_to: [m-upstream]
guidance:
  hitl: pro
concurrency:
  mode: speculative
  sync_on: snapshot
---
```yaml
task_ref: consumer
target: Consume an upstream seam
proof_policy: seam-test-first
scope: [src/lib.rs]
extended_scope: [tests/lib.rs]
symbols: [struct:GateDef]
extended_symbols: [field:GateProfile.commands]
task_depends_on: [m-upstream:producer]
concurrency:
  mode: speculative
  sync_on: snapshot
```
"#,
        "forge-mcp",
    )
    .expect("the production milestone parser should accept the task schema");
    assert_eq!(milestone.required_skills, ["contextunity-forge"]);
    assert_eq!(milestone.tasks[0].depends_on, ["m-upstream:producer"]);
    assert_eq!(milestone.tasks[0].extended_scope, ["tests/lib.rs"]);

    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Navigator,
        ..SubsystemRegistry::default()
    };
    let filtered = filter_subsystem_tools(
        [
            "code_map_overview",
            "forge_read",
            "task_list",
            "task_blackboard",
            "search_docs",
            "session_checkpoint",
        ],
        &registry,
    );
    assert_eq!(filtered, ["code_map_overview", "forge_read", "search_docs"]);
    assert!(
        contextunity_forge_mcp::core::tasks::catalog::disabled_tool_error("task_list", &registry)
            .is_some_and(|error| error.starts_with("FEATURE_DISABLED:"))
    );

    let profile_workspace = crate::common::Workspace::default();
    profile_workspace.write("forge-mcp.yaml", "profile: minimal\n");
    let minimal =
        contextunity_forge_mcp::engine::scanner::load_adapter(profile_workspace.root(), None)
            .unwrap()
            .modules;
    assert!(
        ["codemap", "acdd", "blackboard", "docs", "checkpoints"]
            .iter()
            .all(|module| !minimal.is_enabled(module)),
        "minimal should leave only core tools enabled"
    );
    assert_eq!(
        filter_subsystem_tools(
            [
                "forge_guide",
                "task_list",
                "code_map_overview",
                "forge_read"
            ],
            &minimal
        ),
        ["forge_guide", "forge_read"]
    );
    assert!(
        contextunity_forge_mcp::core::tasks::catalog::disabled_tool_error("forge_read", &minimal)
            .is_none()
    );

    profile_workspace.write("forge-mcp.yaml", "profile: full\n");
    let full =
        contextunity_forge_mcp::engine::scanner::load_adapter(profile_workspace.root(), None)
            .unwrap()
            .modules;
    assert!(["codemap", "acdd", "blackboard", "docs", "checkpoints"]
        .iter()
        .all(|module| full.is_enabled(module)));

    profile_workspace.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  acdd: true\n  docs: true\n",
    );
    let toggles =
        contextunity_forge_mcp::engine::scanner::load_adapter(profile_workspace.root(), None)
            .unwrap()
            .modules;
    assert!(toggles.is_enabled("acdd"));
    assert!(toggles.is_enabled("docs"));
    assert!(!toggles.is_enabled("codemap"));

    profile_workspace.write(
        "forge-mcp.yaml",
        "modules:\n  profile: navigator\n  enabled: [blackboard]\n",
    );
    let legacy =
        contextunity_forge_mcp::engine::scanner::load_adapter(profile_workspace.root(), None)
            .unwrap()
            .modules;
    assert!(legacy.is_enabled("codemap"));
    assert!(legacy.is_enabled("docs"));
    assert!(legacy.is_enabled("blackboard"));

    let validated = contextunity_forge_mcp::cli::validate::run(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        Some(std::path::Path::new(
            "docs/future/references/acdd_profile.v2.yaml.example",
        )),
    )
    .unwrap();
    assert_eq!(validated["valid"], true);

    profile_workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nmilestones: [docs/milestones]\n",
    );
    let valid_manifest = [(
        "docs/milestones/010-valid.md",
        "---\nid: m-valid\nstatus: active\n---\n```yaml\ntask_ref: one\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/one.rs]\n```\n",
    )];
    let overlap_manifest = [(
        "docs/milestones/020-overlap.md",
        "---\nid: m-overlap\nstatus: planned\n---\n```yaml\ntask_ref: one\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/shared.rs]\n```\n```yaml\ntask_ref: two\ntarget: Second task\nproof_policy: seam-test-first\nscope: [src/shared.rs/subfile.rs]\n```\n",
    )];
    let cycle_manifests = [
        (
            "docs/milestones/030-a.md",
            "---\nid: m-a\nstatus: active\n---\n```yaml\ntask_ref: task-a\ntarget: A\nproof_policy: seam-test-first\nscope: [src/a.rs]\ntask_depends_on: [m-b:task-b]\n```\n",
        ),
        (
            "docs/milestones/040-b.md",
            "---\nid: m-b\nstatus: planned\n---\n```yaml\ntask_ref: task-b\ntarget: B\nproof_policy: seam-test-first\nscope: [src/b.rs]\ntask_depends_on: [m-a:task-a]\n```\n",
        ),
    ];
    let validations = [
        (
            "valid workspace",
            valid_manifest.as_slice(),
            true,
            None,
            false,
        ),
        (
            "scope overlap",
            overlap_manifest.as_slice(),
            false,
            Some("MILESTONE_SCOPE_OVERLAP"),
            true,
        ),
        (
            "dependency cycle",
            cycle_manifests.as_slice(),
            false,
            Some("MILESTONE_DAG_CYCLE"),
            false,
        ),
    ];
    for (case, manifests, expected_valid, diagnostic_code, require_location) in validations {
        for &(path, content) in manifests {
            profile_workspace.write(path, content);
        }
        let result =
            contextunity_forge_mcp::cli::validate::run(profile_workspace.root(), None).unwrap();
        assert_eq!(result["valid"], expected_valid, "{case}: {result}");
        if let Some(code) = diagnostic_code {
            let has_expected_diagnostic =
                result["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|diagnostic| {
                        diagnostic["code"] == code
                            && (!require_location
                                || (diagnostic["status"] == "FAIL"
                                    && diagnostic["line"].as_u64().is_some()
                                    && diagnostic["column"].as_u64().is_some()))
                    });
            assert!(
                has_expected_diagnostic,
                "{case}: expected diagnostic {code}: {result}"
            );
        } else {
            assert_eq!(result["scope"], "workspace");
        }
        for &(path, _) in manifests {
            profile_workspace.remove(path);
        }
    }
}
