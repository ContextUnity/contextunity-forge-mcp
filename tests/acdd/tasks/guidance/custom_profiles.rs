use super::*;

#[test]
fn claim_context_exposes_recursive_scheme_schema_from_custom_profile() {
    const PROFILE: &str = r#"
inherit: none
roles:
  builder: {}
  independent_reviewer: {}
gates:
  - id: contract
    role: builder
    proof: none
  - id: schema_input
    role: builder
    proof:
      scheme:
        type: object
        required: [report]
        additionalProperties: false
        properties:
          report:
            type: object
            required: [entries]
            additionalProperties: false
            properties:
              entries:
                type: array
                items:
                  type: object
                  required: [name]
                  additionalProperties: false
                  properties:
                    name:
                      type: string
                      enum: [accepted, rejected]
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: schema_input
    reject_to: schema_input
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: schema_input
    auto_commit: false
"#;
    let workspace = TaskHarness::workspace();
    workspace.write("src/lib.rs", "pub fn schema_gate() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom/profile.yaml\n",
    );
    workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    workspace.write(
        "AGENTS.md",
        "# Task instructions\nLoad the contextunity-forge skill.\n",
    );
    workspace.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    workspace.write("custom/profile.yaml", PROFILE);
    let manifest = MilestoneFixtureBuilder::new("m-scheme")
        .status("active")
        .invariants(&["isolated"])
        .task("schema", &["src/"])
        .task_target("Deliver schema evidence")
        .task_proof_policy("seam-test-first")
        .build();
    workspace.write("docs/010-scheme.md", &manifest);
    let mut store = tasks::store(workspace.root()).unwrap();
    store
        .sync(
            &Milestone::parse(&manifest, "forge-mcp").unwrap(),
            "docs/010-scheme.md",
            workspace.root(),
        )
        .unwrap();
    let id = "forge-mcp/m-scheme:schema".to_owned();
    drop(store);
    let mut h = TaskHarness::attach(&workspace, &id);
    h.advance_to("schema_input").unwrap();
    let task_id = h.id.as_str();
    let claim = TaskHarness::claim_at(h.root(), task_id, "schema_input", "schema-author").unwrap();

    assert_eq!(
        claim["context_bundle"]["proof_schema"],
        json!({
            "type": "object",
            "properties": {
                "report": {
                    "type": "object",
                    "properties": {
                        "entries": {
                            "type": "array",
                            "properties": {},
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "properties": {},
                                        "required": [],
                                        "additionalProperties": null,
                                        "items": null,
                                "enum": ["accepted", "rejected"]
                                    }
                                },
                                "required": ["name"],
                                "additionalProperties": false,
                                "items": null,
                                "enum": []
                            },
                            "required": [],
                            "additionalProperties": null,
                            "enum": []
                        }
                    },
                    "required": ["entries"],
                    "additionalProperties": false,
                    "items": null,
                    "enum": []
                }
            },
            "required": ["report"],
            "additionalProperties": false,
            "items": null,
            "enum": []
        })
    );

    let properties = claim["context_bundle"]["proof_schema"]
        .pointer("/properties/report/properties/entries/items/additionalProperties")
        .expect("array item schema preserves the forbidden-property contract");
    assert_eq!(properties, false);

    let task = TaskHarness::persisted_descriptor(&h.store, task_id).unwrap();
    let before_state = || TaskHarness::task_validation_state(&h.store, task_id).unwrap();
    assert_eq!(task.stage, "schema_input");
    let unchanged = before_state();
    let invalid_proofs = [
        ("nested required field missing", json!({"report":{}})),
        (
            "wrong nested type",
            json!({"report":{"entries":"not-an-array"}}),
        ),
        (
            "enum value rejected",
            json!({"report":{"entries":[{"name":"unknown"}]}}),
        ),
        (
            "array element type rejected",
            json!({"report":{"entries":[42]}}),
        ),
        (
            "forbidden nested property rejected",
            json!({"report":{"entries":[],"extra":true}}),
        ),
    ];
    for (case, proof) in invalid_proofs {
        let mut invalid_evidence = TaskHarness::evidence_for_claim(&claim, json!({}));
        invalid_evidence.proof = json!({"scheme_proof":proof});
        let error = h.submit_evidence(&invalid_evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_EVIDENCE_INVALID"),
            "{case} produced an unexpected error: {error:#}"
        );
        assert_eq!(
            before_state(),
            unchanged,
            "{case} mutated task/gate/claim state"
        );
    }

    let mut accepted_evidence = TaskHarness::evidence_for_claim(&claim, json!({}));
    accepted_evidence.proof = json!({
        "scheme_proof":{"report":{"entries":[{"name":"accepted"}]}}
    });
    let accepted = h.submit_evidence(&accepted_evidence).unwrap();
    assert_eq!(accepted["stage"], "review");
    let submitted = before_state();
    assert_eq!(submitted.status, "ready");
    assert_eq!(submitted.stage, "review");
    assert_eq!(submitted.submissions, unchanged.submissions + 1);
    let stored_gate = TaskHarness::latest_gate_state(&h.store, task_id, "schema_input").unwrap();
    assert_eq!(stored_gate.0, "passed");
    let stored_evidence: Value = serde_json::from_str(stored_gate.1.as_deref().unwrap()).unwrap();
    assert_eq!(
        stored_evidence["proof"]["scheme_proof"]["report"]["entries"][0]["name"],
        "accepted"
    );
}

#[test]
fn claim_context_exposes_named_schema_and_scheme_submission_envelope() {
    const PROFILE: &str = r#"
inherit: none
proofs:
  design:
    schema:
      type: object
      required: [summary]
      properties:
        summary: {type: string}
roles:
  builder: {}
  independent_reviewer: {}
gates:
  - id: contract
    role: builder
    proof: none
  - id: design_input
    role: builder
    proof: design
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: design_input
    fail: design_input
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    fail: design_input
    auto_commit: false
"#;
    let workspace = TaskHarness::workspace();
    workspace.write("src/lib.rs", "pub fn named_schema_gate() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom/profile.yaml\n",
    );
    workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    workspace.write("custom/profile.yaml", PROFILE);
    let manifest = MilestoneFixtureBuilder::new("m-named-schema")
        .status("active")
        .task("schema", &["src/"])
        .task_target("Provide the named schema evidence")
        .build();
    workspace.write("docs/010-named-schema.md", &manifest);
    let mut store = tasks::store(workspace.root()).unwrap();
    store
        .sync(
            &Milestone::parse(&manifest, "forge-mcp").unwrap(),
            "docs/010-named-schema.md",
            workspace.root(),
        )
        .unwrap();
    let task_id = "forge-mcp/m-named-schema:schema";
    drop(store);

    let mut harness = TaskHarness::attach(&workspace, task_id);
    harness.advance_to("design_input").unwrap();
    let claim =
        TaskHarness::claim_at(harness.root(), task_id, "design_input", "schema-author").unwrap();

    assert_eq!(claim["context_bundle"]["proof_schema"]["type"], "object");
    assert_eq!(
        claim["context_bundle"]["proof_schema"]["required"],
        json!(["summary"])
    );
    assert_eq!(claim["workflow_guidance"]["proof_kind"], "design");
    assert_eq!(
        claim["workflow_guidance"]["expected_evidence_template"]["proof"],
        json!({"scheme_proof": {}})
    );
}

#[test]
fn inline_role_mode_adapts_guidance_text() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(
        "docs/AGENTS.md",
        "# Agent Guidance\nUse ContextUnity Forge.\n",
    );
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let profile_yaml = r#"
inherit: none
roles:
  builder: {}
  contract_author: {}
  independent_reviewer:
    mode: inline
    reuse_on_reject: true
    guidance: "Spawn a dedicated reviewer subagent to review candidate diff across contours."
gates:
  - id: contract
    role: contract_author
    proof: none
  - id: build
    role: builder
    proof: none
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
  - id: deliver
    role: independent_reviewer
    proof: delivery
    review_sources: [review]
    reject_to: build
"#;
    root.write("custom_profile.yaml", profile_yaml);

    let milestone_content = r#"---
id: m-inline-role
title: Inline role test
doc_type: contract
status: active
---
# Inline role test

```yaml
task_ref: inline-task
target: Verify inline role guidance
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-inline.md", milestone_content);

    let sync_res = contextunity_forge_mcp::engine::tasks::manage(
        root.root(),
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-inline.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    let inspected = contextunity_forge_mcp::engine::tasks::manage(
        root.root(),
        serde_json::from_value(json!({
            "action": "inspect",
            "task_id": task_id
        }))
        .unwrap(),
    )
    .unwrap();
    // At contract stage, mode is subagent
    assert_eq!(inspected["workflow_guidance"]["mode"], "subagent");

    // Advance to review stage
    let mut h = TaskHarness::attach(&root, task_id);
    h.advance_to("review").unwrap();

    // Now claim review stage: mode must be inline and guidance must be adapted!
    let review_claim =
        TaskHarness::claim_at(h.root(), task_id, "review", "inline-reviewer").unwrap();

    let guidance = &review_claim["workflow_guidance"];
    assert_eq!(guidance["role"], "independent_reviewer");
    assert_eq!(guidance["mode"], "inline");
    let role_guide = guidance["role_guide"].as_str().unwrap();
    assert!(
        role_guide.contains(
            "Execute inline in the current agent to review candidate diff across contours."
        ),
        "expected adapted inline role guide, got: {role_guide}"
    );
    assert!(!role_guide.contains("Spawn a dedicated"));
}
