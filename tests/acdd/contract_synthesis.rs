use contextunity_forge_mcp::{
    core::tasks::{
        AcddErrorCode, ContractStatus, ContractType, Milestone, MilestoneContractEntry,
        MilestoneContractMatrix,
    },
    engine::milestones::verify_contract_matrix_parity,
};

#[test]
fn missing_dependency_relationship_is_rejected_with_a_stable_diagnostic() {
    let milestone = Milestone::parse(
        r#"---
id: acdd-contract-parity
repository: forge-mcp
project: forge-mcp
---
```yaml
task_ref: producer
target: Produce a task contract
proof_policy: seam-test-first
scope: [src/producer.rs]
```
```yaml
task_ref: consumer
target: Consume a task contract
proof_policy: seam-test-first
scope: [src/consumer.rs]
depends_on: [producer]
```
"#,
        "forge-mcp",
    )
    .expect("the milestone fixture is valid");
    let make_entry = |source_task: &str,
                      target_task: &str,
                      contract_type: ContractType,
                      contract_ref: &str,
                      revision: u64| MilestoneContractEntry {
        milestone_id: milestone.id.clone(),
        source_task: source_task.to_owned(),
        target_task: target_task.to_owned(),
        contract_type,
        contract_ref: contract_ref.to_owned(),
        revision,
        status: ContractStatus::Synthesized,
    };
    let make_matrix = |milestone_id: &str, entries| MilestoneContractMatrix {
        milestone_id: milestone_id.to_owned(),
        entries,
    };
    let matching_edge = make_entry(
        "producer",
        "consumer",
        ContractType::SeamDependency,
        "fn:contract_boundary",
        1,
    );
    let invalid_matrices = [
        ("missing dependency", make_matrix(&milestone.id, Vec::new())),
        (
            "unknown task",
            make_matrix(
                &milestone.id,
                vec![make_entry(
                    "unknown",
                    "consumer",
                    ContractType::SeamDependency,
                    "fn:contract_boundary",
                    1,
                )],
            ),
        ),
        (
            "empty reference",
            make_matrix(
                &milestone.id,
                vec![make_entry(
                    "producer",
                    "consumer",
                    ContractType::SeamDependency,
                    " ",
                    1,
                )],
            ),
        ),
        (
            "zero revision",
            make_matrix(
                &milestone.id,
                vec![make_entry(
                    "producer",
                    "consumer",
                    ContractType::SeamDependency,
                    "fn:contract_boundary",
                    0,
                )],
            ),
        ),
        (
            "wrong dependency direction",
            make_matrix(
                &milestone.id,
                vec![make_entry(
                    "consumer",
                    "producer",
                    ContractType::SeamDependency,
                    "fn:contract_boundary",
                    1,
                )],
            ),
        ),
        (
            "wrong relationship kind",
            make_matrix(
                &milestone.id,
                vec![make_entry(
                    "producer",
                    "consumer",
                    ContractType::SharedType,
                    "fn:contract_boundary",
                    1,
                )],
            ),
        ),
        (
            "wrong matrix milestone",
            make_matrix("another-milestone", vec![matching_edge.clone()]),
        ),
        (
            "wrong entry milestone",
            make_matrix(
                &milestone.id,
                vec![MilestoneContractEntry {
                    milestone_id: "another-milestone".to_owned(),
                    ..matching_edge.clone()
                }],
            ),
        ),
        (
            "duplicate row key",
            make_matrix(
                &milestone.id,
                vec![matching_edge.clone(), matching_edge.clone()],
            ),
        ),
    ];
    for (case, matrix) in invalid_matrices {
        let error = match verify_contract_matrix_parity(&milestone, &matrix) {
            Err(error) => error,
            Ok(()) => panic!("{case} must be rejected"),
        };
        assert_eq!(
            error.code,
            AcddErrorCode::ContractMatrixMismatch,
            "wrong diagnostic for {case}"
        );
        assert_eq!(error.code.as_str(), "CONTRACT_MATRIX_MISMATCH");
    }
    assert!(
        verify_contract_matrix_parity(&milestone, &make_matrix(&milestone.id, vec![matching_edge]))
            .is_ok(),
        "a correctly directed, versioned dependency should pass"
    );
    let encoded = serde_json::to_value(make_entry(
        "producer",
        "consumer",
        ContractType::SeamDependency,
        "fn:contract_boundary",
        1,
    ))
    .expect("contract entries serialize");
    assert_eq!(encoded["contract_type"], "seam_dependency");
    assert_eq!(encoded["status"], "synthesized");
    assert_eq!(
        serde_json::to_value(AcddErrorCode::TaskSeamStale).unwrap(),
        "TASK_SEAM_STALE"
    );

    let cycle_error = Milestone::parse(
        r#"---
id: acdd-cycle
repository: forge-mcp
project: forge-mcp
---
```yaml
task_ref: producer
target: Produce a task contract
proof_policy: seam-test-first
scope: [src/producer.rs]
depends_on: [consumer]
```
```yaml
task_ref: consumer
target: Consume a task contract
proof_policy: seam-test-first
scope: [src/consumer.rs]
depends_on: [producer]
```
"#,
        "forge-mcp",
    )
    .expect_err("cyclic task dependencies must fail");
    assert!(
        cycle_error.to_string().contains("MILESTONE_DAG_CYCLE"),
        "cycle errors must use the shared stable diagnostic"
    );
}
