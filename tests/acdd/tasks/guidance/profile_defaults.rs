use super::*;

#[test]
fn declarative_profile_compiles_defaults_and_respects_role_overrides() {
    use contextunity_forge_mcp::core::tasks::profile::{
        compiled, load_for_workspace, parse_profile, validate_profile_document, RoleDef,
        RoleExecutionMode,
    };

    let role_defaults = RoleDef::default();
    assert_eq!(role_defaults.mode, None);
    assert_eq!(role_defaults.reuse_on_reject, None);
    assert_eq!(role_defaults.effective_mode(), RoleExecutionMode::Subagent);
    assert!(role_defaults.effective_reuse_on_reject());

    // 1. Embedded roles define the default execution and rejection policies without
    //    duplicating ModelSpec's model and reasoning fields at the role level.
    let default_profile = compiled();
    let task_gates = default_profile
        .gates
        .iter()
        .find_map(|g| g.tasks.as_ref())
        .map(|t| &t.gates)
        .expect("task gates exist");
    for required_id in ["contract", "build", "review", "deliver"] {
        assert!(
            task_gates.iter().any(|g| g.id == required_id),
            "expected required gate '{required_id}' to exist"
        );
    }
    let seam_gate = task_gates
        .iter()
        .find(|g| g.id == "seam")
        .expect("seam gate exists");
    assert_eq!(seam_gate.when.as_deref(), Some("seam"));

    let reviewer_role = default_profile
        .roles
        .get("independent_reviewer")
        .expect("reviewer role exists");
    let reviewer_value = serde_json::to_value(reviewer_role).unwrap();
    assert_eq!(reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(reviewer_value["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(
        reviewer_value["models"],
        serde_json::json!([{"model":"sol-6.1","reasoning":"xhigh"}])
    );
    assert!(
        reviewer_value.get("model").is_none(),
        "model belongs to ModelSpec"
    );
    assert!(
        reviewer_value.get("reasoning").is_none(),
        "reasoning belongs to ModelSpec"
    );
    assert!(
        reviewer_role.guidance.is_some(),
        "default reviewer must provide textual guidance"
    );

    // 2. Strict deny_unknown_fields fails closed on unknown YAML keys
    let invalid_yaml = "unknown_field: true\ngates: []\n";
    let parse_err = parse_profile(invalid_yaml).unwrap_err();
    assert!(
        parse_err
            .to_string()
            .contains("ACDD_PROFILE_SCHEMA_INVALID"),
        "expected ACDD_PROFILE_SCHEMA_INVALID error on unknown field, got: {parse_err}"
    );

    // 3. Repository override replaces mode, rejection policy, and ordered models.
    let ws = TaskHarness::workspace_with_file(
        ".forge/acdd/profile.yaml",
        "roles:\n  independent_reviewer:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: sol-6.1\n        reasoning: high\n      - model: sol-6.0\n        reasoning: medium\n",
    );

    let loaded = load_for_workspace(ws.root()).unwrap();
    let loaded_reviewer = loaded.roles.get("independent_reviewer").unwrap();
    let loaded_reviewer_value = serde_json::to_value(loaded_reviewer).unwrap();
    assert_eq!(loaded_reviewer_value["mode"], serde_json::json!("inline"));
    assert_eq!(
        loaded_reviewer_value["reuse_on_reject"],
        serde_json::json!(false)
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["model"],
        serde_json::json!("sol-6.1")
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("high")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["model"],
        serde_json::json!("sol-6.0")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(loaded_reviewer_value.get("model").is_none());
    assert!(loaded_reviewer_value.get("reasoning").is_none());
    assert!(
        loaded_reviewer.guidance.is_some(),
        "the base guidance is inherited"
    );
    // Other gates and roles remain preserved from defaults
    let loaded_task_gates = loaded
        .gates
        .iter()
        .find_map(|g| g.tasks.as_ref())
        .map(|t| &t.gates)
        .expect("task gates exist");
    for expected_gate in ["contract", "build", "review", "deliver"] {
        assert!(
            loaded_task_gates.iter().any(|g| g.id == expected_gate),
            "expected task gate '{expected_gate}' to exist in loaded profile"
        );
    }
    assert!(loaded.roles.contains_key("builder"));

    // 4. Omitted role overrides inherit the base defaults and a model list replaces as a unit.
    ws.write(
        "custom/team_profile.yaml",
        "roles:\n  independent_reviewer:\n    models:\n      - model: custom-model\n        reasoning: medium\n",
    );
    ws.write("forge-mcp.yaml", "acdd_profile: custom/team_profile.yaml\n");
    let loaded_custom = load_for_workspace(ws.root()).unwrap();
    let custom_reviewer = loaded_custom.roles.get("independent_reviewer").unwrap();
    let custom_reviewer_value = serde_json::to_value(custom_reviewer).unwrap();
    assert_eq!(custom_reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(
        custom_reviewer_value["reuse_on_reject"],
        serde_json::json!(true)
    );
    assert_eq!(custom_reviewer_value["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        custom_reviewer_value["models"][0]["model"],
        serde_json::json!("custom-model")
    );
    assert_eq!(
        custom_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(custom_reviewer_value.get("model").is_none());
    assert!(custom_reviewer_value.get("reasoning").is_none());

    // Direct field-wise merge keeps base fields when an override omits them or
    // supplies an empty model list, then replaces the ordered models as a unit.
    let mut base = parse_profile(
        "roles:\n  builder:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: base-model\n        reasoning: high\n    guidance: base guidance\n",
    )
    .unwrap();
    base.merge_override(
        parse_profile(
            "roles:\n  builder:\n    mode: null\n    reuse_on_reject: null\n    models: []\n",
        )
        .unwrap(),
    );
    let inherited = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(inherited["mode"], serde_json::json!("inline"));
    assert_eq!(inherited["reuse_on_reject"], serde_json::json!(false));
    assert_eq!(
        inherited["models"][0]["model"],
        serde_json::json!("base-model")
    );
    assert_eq!(inherited["guidance"], serde_json::json!("base guidance"));
    base.merge_override(parse_profile(
        "roles:\n  builder:\n    mode: subagent\n    reuse_on_reject: true\n    models:\n      - model: replacement-model\n        reasoning: medium\n",
    )
    .unwrap());
    let replaced = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(replaced["mode"], serde_json::json!("subagent"));
    assert_eq!(replaced["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(replaced["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        replaced["models"][0]["model"],
        serde_json::json!("replacement-model")
    );
    assert_eq!(replaced["guidance"], serde_json::json!("base guidance"));

    // String model notation parses model:reasoning and bare model strings
    let string_models = parse_profile(
        "roles:\n  builder:\n    models:\n      - 'gpt6-luna:xhigh'\n      - 'gemini-3.8:high'\n      - 'composer-2.5'\n",
    )
    .unwrap();
    let builder_models = &string_models.roles.get("builder").unwrap().models;
    assert_eq!(builder_models.len(), 3);
    assert_eq!(builder_models[0].model, "gpt6-luna");
    assert_eq!(builder_models[0].reasoning.as_deref(), Some("xhigh"));
    assert_eq!(builder_models[0].formatted(), "gpt6-luna:xhigh");
    assert_eq!(builder_models[1].formatted(), "gemini-3.8:high");
    assert_eq!(builder_models[2].formatted(), "composer-2.5");

    // Invalid string models with empty segments fail closed
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - 'model:'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ':high'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ''\n").is_err());
    // Map models with unknown fields fail closed
    assert!(parse_profile(
        "roles:\n  builder:\n    models:\n      - model: foo\n        unknown_field: bar\n"
    )
    .is_err());

    // The documented reference profile example loads and validates cleanly
    let example_profile_text =
        std::fs::read_to_string("skills/contextunity-forge/references/acdd_profile.yaml.example")
            .expect("example profile file must exist");
    let parsed_example = parse_profile(&example_profile_text)
        .expect("example profile must parse and validate without error");
    assert!(!parsed_example.roles.is_empty());
    // exactly one terminal delivery gate instead of accepting an incomplete workflow.
    let profile_workspace = TaskHarness::workspace_with_file(
        ".forge/acdd/profile.yaml",
        "gates:\n  - id: inspect\n    role: builder\n    proof: none\n",
    );
    let invalid_profile = load_for_workspace(profile_workspace.root());
    assert!(
        invalid_profile.is_err(),
        "a merged profile without exactly one terminal delivery gate must fail closed"
    );

    let valid_gates = r#"gates:
  - id: work
    role: builder
    proof: none
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: work
    independent_from: work
    contours: guidance.contours
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: work
"#;
    let later_gate = "  - id: later\n    role: builder\n    proof: none\n";
    let assert_profile_error = |case: &str, yaml: &str, expected: &str| {
        profile_workspace.write(".forge/acdd/profile.yaml", yaml);
        let error = match load_for_workspace(profile_workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains(expected),
            "{case} expected diagnostic {expected:?}, got: {detail}"
        );
    };
    let invalid_token = valid_gates
        .replacen("id: work", "id: work/v1", 1)
        .replacen("reject_to: work", "reject_to: work/v1", 2)
        .replacen("independent_from: work", "independent_from: work/v1", 1);
    assert_profile_error(
        "invalid gate token",
        &invalid_token,
        "gate id 'work/v1' must be a non-empty token",
    );
    let gates_without_receipt_review = valid_gates.replace("    receipt_review: review\n", "");
    let missing_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [absent]",
        1,
    );
    assert_profile_error(
        "missing review source",
        &missing_review_source,
        "review_sources must reference strictly prior review gates",
    );
    let non_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [work]",
        1,
    );
    assert_profile_error(
        "non-review review source",
        &non_review_source,
        "review source 'work' is not a review gate",
    );
    let cases = [
        (
            "duplicate gate id",
            valid_gates.replacen(
                "  - id: review\n    role: independent_reviewer\n    proof: review",
                "  - id: work\n    role: builder\n    proof: none\n  - id: review\n    role: independent_reviewer\n    proof: review",
                1,
            ),
        ),
        (
            "forward reject_to",
            valid_gates
                .replacen("reject_to: work", "reject_to: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing reject_to",
            valid_gates.replacen("reject_to: work", "reject_to: absent", 1),
        ),
        (
            "forward independent_from",
            valid_gates
                .replacen("independent_from: work", "independent_from: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing independent_from",
            valid_gates.replacen("independent_from: work", "independent_from: absent", 1),
        ),
        (
            "missing receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: absent", 1),
        ),
        (
            "non-review receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: work", 1),
        ),
        (
            "undefined role",
            valid_gates.replacen("role: builder", "role: missing_role", 1),
        ),
        (
            "undefined contour set",
            valid_gates.replacen("contours: guidance.contours", "contours: missing_contours", 1),
        ),
        (
            "empty command value",
            format!("commands:\n  blank: '  '\n{valid_gates}"),
        ),
        (
            "empty command registry name",
            format!("commands:\n  '': echo\n{valid_gates}"),
        ),
        (
            "array schema without items",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: array",
                1,
            ),
        ),
        (
            "schema required property is undefined",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        required: [missing]\n        properties:\n          nested:\n            type: string",
                1,
            ),
        ),
        (
            "invalid nested schema type",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        properties:\n          nested:\n            type: unsupported",
                1,
            ),
        ),
    ];
    for (case, yaml) in cases {
        profile_workspace.write(".forge/acdd/profile.yaml", &yaml);
        let error = match load_for_workspace(profile_workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains("ACDD_PROFILE_INVALID")
                || detail.contains("ACDD_PROFILE_SCHEMA_INVALID")
                || detail.contains("GATE_INVALID_CONFIGURATION"),
            "{case} failed for an unexpected reason: {detail}"
        );
    }

    let empty_registry = parse_profile(
        "roles:\n  builder: {}\n  independent_reviewer: {}\ncommands: {}\ngates:\n  - id: work\n    role: builder\n    proof: command\n  - id: review\n    role: independent_reviewer\n    proof: review\n  - id: deliver\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [review]\n",
    )
    .unwrap();
    let error = empty_registry.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires a non-empty commands registry"),
        "unexpected empty-registry validation error: {error:#}"
    );

    let compiled_empty_catalog = validate_profile_document(
        "inherit: none\nroles:\n  worker: {}\ncommands: {}\ngates:\n  - id: build\n    role: worker\n    proof: none\n  - id: review\n    role: worker\n    proof: review\n    reject_to: build\n    independent_from: build\n  - id: deliver\n    role: worker\n    proof: delivery\n    review_sources: [review]\n",
    )
    .expect("an explicitly empty catalog should compile when no gate uses a command");
    assert!(
        compiled_empty_catalog.commands.is_empty(),
        "an explicit empty command catalog must clear embedded commands"
    );

    let clean_room_yaml = r#"
inherit: none
version: 2
roles:
  worker: {}
commands: {}
gates:
  - id: build
    role: worker
    proof: none
  - id: review
    role: worker
    proof: review
    reject_to: build
    independent_from: build
  - id: deliver
    role: worker
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: build
"#;
    profile_workspace.write(".forge/acdd/profile.yaml", clean_room_yaml);

    let clean_room = load_for_workspace(profile_workspace.root());
    assert!(
        clean_room.is_ok(),
        "inherit: none should compile an isolated profile with an empty commands catalog: {clean_room:?}"
    );
    let clean_room = clean_room.unwrap();
    assert_eq!(
        clean_room
            .roles
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["worker"]
    );
    assert!(clean_room.commands.is_empty());
    assert!(clean_room.proofs.is_empty());
    assert!(clean_room.guidance.is_empty());
    assert!(clean_room.policies.is_empty());
    assert!(clean_room.contour_set(None).is_none());
    assert!(clean_room.gate_templates.is_empty());

    let compiled_clean_room = validate_profile_document(clean_room_yaml)
        .expect("the production profile compiler should honor inherit: none");
    assert!(compiled_clean_room.commands.is_empty());
    assert!(!compiled_clean_room.roles.contains_key("contract_author"));
}

#[test]
fn inherit_default_deep_merges_gate_overrides_and_loads_repository_overlay() {
    use contextunity_forge_mcp::core::tasks::profile::{
        load_for_workspace, validate_profile_document,
    };
    use std::path::Path;

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_profile = load_for_workspace(repo_root)
        .expect(".forge/acdd/profile.yaml repository overlay should load cleanly");

    // 1. Inherited default gates exist with overlay commands bound by ID
    let contract = repo_profile
        .find_gate("contract")
        .expect("contract gate exists");
    assert_eq!(contract.command.as_deref(), Some("test"));
    assert_eq!(contract.role.as_deref(), Some("contract_author"));

    let build = repo_profile.find_gate("build").expect("build gate exists");
    assert_eq!(build.command.as_deref(), Some("rust-check-and-test"));
    assert_eq!(build.role.as_deref(), Some("builder"));

    let verify = repo_profile
        .find_gate("verify")
        .expect("verify gate exists");
    assert_eq!(verify.command.as_deref(), Some("check_full_repo"));
    assert!(
        verify.role.is_none(),
        "verify must not have role in neutral profile"
    );

    let blueprint = repo_profile
        .find_gate("blueprint_synthesis")
        .expect("blueprint_synthesis container exists");
    assert!(
        blueprint.gates.iter().any(|g| (g.id == "synthesize"
            || g.id == "blueprint_synthesis.synthesize")
            && g.role.as_deref() == Some("architect")),
        "{:?}",
        blueprint.gates
    );
    assert!(
        blueprint.gates.iter().any(
            |g| (g.id == "review" || g.id == "blueprint_synthesis.review")
                && g.role.as_deref() == Some("independent_reviewer")
        ),
        "{:?}",
        blueprint.gates
    );

    // 2. Custom overlay deep-merge preserves unspecified fields while overriding specific gates
    let overlay_yaml = r#"
version: 2
inherit: default
commands:
  custom-check: "cargo check"
gates:
  - id: verify
    command: custom-check
"#;
    let merged = validate_profile_document(overlay_yaml)
        .expect("overlay with inherit: default should deep-merge cleanly");
    assert_eq!(
        merged
            .find_gate("verify")
            .and_then(|g| g.command.as_deref()),
        Some("custom-check")
    );
    assert_eq!(
        merged.find_gate("contract").and_then(|g| g.role.as_deref()),
        Some("contract_author")
    );
}
