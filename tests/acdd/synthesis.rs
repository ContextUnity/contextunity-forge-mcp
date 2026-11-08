use clap::Parser;
use contextunity_forge_mcp::engine::milestones::init_from_manifest;
use contextunity_forge_mcp::engine::tasks::blackboard;
use contextunity_forge_mcp::{
    core::tasks::{AcddErrorCode, ContractType, Milestone},
    db::tasks_store::TasksStore,
    engine::milestones::{
        extract_shared_helpers, synthesize_milestone_contracts, validate_wave_dag,
    },
    engine::tasks::{BlackboardAction, BlackboardRequest},
};
use std::path::Path;

#[test]
fn wave_validation_orders_dependencies_and_rejects_parallel_scope_overlap() {
    let valid = Milestone::parse(
        r#"---
id: acdd-wave-contract
repository: forge-mcp
project: forge-mcp
---
```yaml
task_ref: provider
target: Provide a shared seam
proof_policy: seam-test-first
scope: [src/provider.rs]
symbols: [fn:shared_helper, struct:SharedContract]
```
```yaml
task_ref: independent
target: Run independently
proof_policy: seam-test-first
scope: [src/independent.rs]
symbols: [fn:shared_helper, struct:SharedContract]
```
```yaml
task_ref: consumer
target: Consume the shared seam
proof_policy: seam-test-first
scope: [src/consumer.rs]
depends_on: [provider]
```
"#,
        "forge-mcp",
    )
    .expect("the milestone fixture is valid");
    let waves = validate_wave_dag(&valid).expect("the disjoint DAG is valid");
    assert_eq!(
        waves,
        vec![
            vec!["provider".to_owned(), "independent".to_owned()],
            vec!["consumer".to_owned()],
        ],
        "dependency-free tasks retain manifest order before their consumers"
    );

    let shared = extract_shared_helpers(&valid);
    assert_eq!(shared.len(), 2);
    assert!(shared.iter().any(|entry| {
        entry.contract_ref == "fn:shared_helper"
            && entry.contract_type == ContractType::SharedHelper
            && entry.source_task == "provider"
            && entry.target_task == "independent"
    }));
    assert!(shared.iter().any(|entry| {
        entry.contract_ref == "struct:SharedContract"
            && entry.contract_type == ContractType::SharedType
    }));

    let matrix = synthesize_milestone_contracts(&valid)
        .expect("seam dependencies and shared symbols form a valid contract matrix");
    assert!(matrix.entries.iter().any(|entry| {
        entry.contract_type == ContractType::SeamDependency
            && entry.source_task == "provider"
            && entry.target_task == "consumer"
    }));

    let mut cyclic = valid.clone();
    cyclic
        .tasks
        .iter_mut()
        .find(|task| task.task_ref == "provider")
        .expect("provider task exists")
        .depends_on
        .push("consumer".to_owned());
    let cycle = validate_wave_dag(&cyclic).expect_err("cycles must have a stable diagnostic");
    assert_eq!(cycle.code, AcddErrorCode::MilestoneDagCycle);

    let overlapping = Milestone::parse(
        r#"---
id: acdd-overlapping-wave-contract
repository: forge-mcp
project: forge-mcp
---
```yaml
task_ref: first
target: First writer
proof_policy: seam-test-first
scope: [src/shared]
```
```yaml
task_ref: second
target: Second writer
proof_policy: seam-test-first
scope: [src/shared/nested.rs]
```
"#,
        "forge-mcp",
    )
    .expect("the overlapping-scope fixture is valid as a manifest");
    let error =
        validate_wave_dag(&overlapping).expect_err("same-wave owners of one path must be rejected");
    assert_eq!(error.code, AcddErrorCode::TaskScopeInvalid);
}

#[test]
fn milestone_init_preflight_checks_skills_before_persisting_task_state() {
    let workspace = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\ntask_repository: forge-mcp\nmilestones: [docs/milestones]\n",
    );
    let secondary = workspace.root().join("secondary");
    std::fs::create_dir_all(&secondary).expect("secondary participant directory is created");
    crate::acdd::support::init_git(&secondary);
    let manifest = "---\nid: preflight-fixture\nrepository: forge-mcp\nproject: forge-mcp\nstatus: active\nrequired_skills: [fixture-skill]\nrepositories:\n  - forge-mcp\n  - name: secondary\n    path: secondary\n---\n```yaml\ntask_ref: first\ntarget: Provide the shared seam\nproof_policy: seam-test-first\nscope: [src/first.rs]\nsymbols: [fn:shared_helper]\n```\n```yaml\ntask_ref: second\ntarget: Consume the shared seam\nproof_policy: seam-test-first\nscope: [src/second.rs]\nsymbols: [fn:shared_helper]\ndepends_on: [first]\n```\n";
    workspace.write("docs/milestones/001-preflight.md", manifest);

    let missing = init_from_manifest(
        workspace.root(),
        Path::new("docs/milestones/001-preflight.md"),
        true,
    )
    .expect_err("missing skills must fail preflight");
    assert!(missing.to_string().contains("MILESTONE_PREFLIGHT_FAILED"));
    assert!(
        !workspace.root().join(".forge/tasks.sqlite").exists(),
        "skill preflight runs before SQLite initialization"
    );
    assert!(
        !workspace.root().join(".worktrees").exists(),
        "in-place preflight does not create worktree directories"
    );

    workspace.write(
        ".agents/skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\n---\n",
    );
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\ntask_repository: forge-mcp\nmilestones: [docs/milestones]\nmodules:\n  profile: minimal\n",
    );
    workspace.write(
        ".forge/acdd/profile.yaml",
        "modules:\n  required: [blackboard]\n",
    );
    let disabled_requirement = init_from_manifest(
        workspace.root(),
        Path::new("docs/milestones/001-preflight.md"),
        true,
    )
    .expect_err("a profile requirement disabled by the adapter must fail preflight");
    assert!(disabled_requirement
        .to_string()
        .contains("FEATURE_REQUIRED_BY_PROFILE"));
    assert!(
        !workspace.root().join(".forge/tasks.sqlite").exists(),
        "profile module validation runs before SQLite initialization"
    );
    assert!(
        !workspace.root().join(".worktrees").exists(),
        "profile module validation runs before worktree creation"
    );

    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\ntask_repository: forge-mcp\nmilestones: [docs/milestones]\n",
    );
    let overflow_manifest = manifest.replace(
        "task_ref: first\n",
        "task_ref: first\ncontract_revision: 18446744073709551615\n",
    );
    workspace.write("docs/milestones/001-preflight.md", &overflow_manifest);
    let overflow = init_from_manifest(
        workspace.root(),
        Path::new("docs/milestones/001-preflight.md"),
        true,
    )
    .expect_err("a SQLite-unrepresentable task revision must fail preflight");
    assert!(overflow
        .to_string()
        .contains("revision exceeds SQLite integer range"));
    assert!(
        !workspace.root().join(".forge/tasks.sqlite").exists(),
        "revision validation runs before SQLite initialization"
    );
    assert!(
        !workspace.root().join(".worktrees").exists(),
        "revision validation runs before worktree creation"
    );

    workspace.write("docs/milestones/001-preflight.md", manifest);
    let initialized = init_from_manifest(
        workspace.root(),
        Path::new("docs/milestones/001-preflight.md"),
        true,
    )
    .expect("installed skills permit in-place preflight initialization");
    assert_eq!(initialized["milestone_id"], "preflight-fixture");
    assert_eq!(initialized["in_place"], true);
    assert_eq!(initialized["contract_count"], 2);
    assert!(workspace.root().join(".forge/tasks.sqlite").is_file());
    assert!(
        !secondary.join(".worktrees").exists(),
        "in-place preflight validates participants without creating worktrees"
    );
    let store = TasksStore::open_project(
        &workspace.root().join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .expect("preflight initializes the project task store");
    let matrix = store
        .list_contract_matrix("preflight-fixture")
        .expect("preflight persists the synthesized contract matrix");
    assert_eq!(matrix.entries.len(), 2);
    let published = blackboard(
        workspace.root(),
        BlackboardRequest {
            action: BlackboardAction::Read,
            scope: Some(contextunity_forge_mcp::engine::tasks::BlackboardScope::Milestone),
            milestone_ref: Some("docs/milestones/001-preflight.md".into()),
            task_id: None,
            recipient_task_id: None,
            subtask_ref: None,
            message_id: None,
            author: None,
            topic: None,
            gate: None,
            payload: None,
            limit: None,
            offset: None,
        },
        "test",
    )
    .expect("preflight publishes its bounded matrix summary");
    let summary_row = published["messages"]
        .as_array()
        .expect("milestone read returns persisted messages")
        .iter()
        .find(|message| message["author"] == "milestone-init")
        .expect("the synthesized matrix summary is on the milestone blackboard");
    assert_eq!(summary_row["topic"], "draft");
    let inspected_summary = blackboard(
        workspace.root(),
        BlackboardRequest {
            action: BlackboardAction::Inspect,
            scope: None,
            milestone_ref: None,
            task_id: None,
            recipient_task_id: None,
            subtask_ref: None,
            message_id: summary_row["id"].as_u64(),
            author: None,
            topic: None,
            gate: None,
            payload: None,
            limit: None,
            offset: None,
        },
        "test",
    )
    .expect("the bounded matrix summary can be read back");
    let summary_payload: serde_json::Value =
        serde_json::from_str(inspected_summary["message"]["payload"].as_str().unwrap())
            .expect("the bounded summary payload is JSON");
    assert_eq!(summary_payload["contract_count"], 2);

    let source_task = "forge-mcp/preflight-fixture:first";
    let recipient = "forge-mcp/preflight-fixture:second";
    let targeted: BlackboardRequest = serde_json::from_value(serde_json::json!({
        "action": "post",
        "scope": "task",
        "task_id": source_task,
        "recipient_task_id": recipient,
        "milestone_ref": "docs/milestones/001-preflight.md",
        "topic": "notes",
        "payload": "Targeted preflight note",
    }))
    .expect("recipient_task_id is accepted as the task ownership selector");
    let posted = blackboard(workspace.root(), targeted, "test")
        .expect("recipient task owns its blackboard row");
    let recipient_store_read = store
        .blackboard_read(recipient, Some("notes"), None)
        .expect("task store reads messages addressed to the recipient");
    assert!(recipient_store_read
        .iter()
        .any(|message| message.id == posted["id"].as_u64().unwrap()));
    let inspected = blackboard(
        workspace.root(),
        BlackboardRequest {
            action: BlackboardAction::Inspect,
            scope: None,
            milestone_ref: None,
            task_id: None,
            recipient_task_id: None,
            subtask_ref: None,
            message_id: posted["id"].as_u64(),
            author: None,
            topic: None,
            gate: None,
            payload: None,
            limit: None,
            offset: None,
        },
        "test",
    )
    .expect("addressed blackboard message can be inspected");
    assert!(inspected["message"]
        .to_string()
        .contains("Targeted preflight note"));
    assert_eq!(inspected["message"]["task_id"], source_task);
    assert_eq!(inspected["message"]["recipient_task_id"], recipient);
    let recipient_read = blackboard(
        workspace.root(),
        BlackboardRequest {
            action: BlackboardAction::Read,
            scope: Some(contextunity_forge_mcp::engine::tasks::BlackboardScope::Task),
            milestone_ref: Some("docs/milestones/001-preflight.md".into()),
            task_id: Some(recipient.into()),
            recipient_task_id: None,
            subtask_ref: None,
            message_id: None,
            author: None,
            topic: Some("notes".into()),
            gate: None,
            payload: None,
            limit: None,
            offset: None,
        },
        "test",
    )
    .expect("recipient task reads its addressed messages");
    assert!(recipient_read["messages"]
        .as_array()
        .expect("task read returns message summaries")
        .iter()
        .any(|message| message["id"] == posted["id"]));

    #[derive(Parser)]
    struct TaskCli {
        #[command(subcommand)]
        command: contextunity_forge_mcp::cli::task::TaskCommand,
    }
    let parsed = TaskCli::try_parse_from([
        "forge",
        "blackboard",
        "post",
        source_task,
        "--scope",
        "task",
        "--milestone-ref",
        "docs/milestones/001-preflight.md",
        "--recipient-task-id",
        recipient,
        "--topic",
        "notes",
        "--payload",
        "CLI targeted preflight note",
    ])
    .expect("task blackboard CLI exposes a separate recipient option");
    let cli_post = contextunity_forge_mcp::cli::task::run(workspace.root(), parsed.command)
        .expect("CLI routes the recipient to the blackboard owner");
    let cli_inspected = blackboard(
        workspace.root(),
        BlackboardRequest {
            action: BlackboardAction::Inspect,
            scope: None,
            milestone_ref: None,
            task_id: None,
            recipient_task_id: None,
            subtask_ref: None,
            message_id: cli_post["id"].as_u64(),
            author: None,
            topic: None,
            gate: None,
            payload: None,
            limit: None,
            offset: None,
        },
        "test",
    )
    .expect("CLI-created message can be inspected");
    assert_eq!(cli_inspected["message"]["task_id"], source_task);
    assert_eq!(cli_inspected["message"]["recipient_task_id"], recipient);

    let scaffolded = init_from_manifest(
        workspace.root(),
        Path::new("docs/milestones/001-preflight.md"),
        false,
    )
    .expect("preflight creates a repository worktree after validation");
    assert_eq!(scaffolded["repositories"].as_array().unwrap().len(), 2);
    assert!(scaffolded["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .all(|participant| participant["created"] == true));
    assert!(workspace
        .root()
        .join(".worktrees/preflight-fixture")
        .is_dir());
    assert!(secondary.join(".worktrees/preflight-fixture").is_dir());

    #[derive(Parser)]
    struct MilestoneCli {
        #[command(subcommand)]
        command: contextunity_forge_mcp::cli::milestone::MilestoneCommand,
    }
    let parsed = MilestoneCli::try_parse_from([
        "forge",
        "draft",
        "--slug",
        "draft-fixture",
        "--title",
        "Draft fixture",
        "--desc",
        "Exercise the public milestone draft command.",
    ])
    .expect("milestone draft options are part of the public CLI schema");
    let draft = contextunity_forge_mcp::cli::milestone::run(workspace.root(), parsed.command)
        .expect("milestone draft command creates a numbered contract");
    assert_eq!(draft["id"], "m-draft-fixture");
    assert!(workspace
        .root()
        .join(draft["path"].as_str().unwrap())
        .is_file());

    let assert_atomic_failure = |_fixture_name: &str, trigger_sql: &str| {
        let atomic_workspace =
            crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
        atomic_workspace.write(
            "forge-mcp.yaml",
            "tasks_db: .forge/tasks.sqlite\ntask_repository: forge-mcp\nmilestones: [docs/milestones]\n",
        );
        atomic_workspace.write(
            ".agents/skills/fixture-skill/SKILL.md",
            "---\nname: fixture-skill\n---\n",
        );
        let atomic_secondary = atomic_workspace.root().join("secondary");
        std::fs::create_dir_all(&atomic_secondary).expect("the secondary atomicity fixture exists");
        crate::acdd::support::init_git(&atomic_secondary);
        atomic_workspace.write("docs/milestones/001-preflight.md", manifest);

        let database = atomic_workspace.root().join(".forge/tasks.sqlite");
        let store = TasksStore::open_project(&database, "forge-mcp", "forge-mcp")
            .expect("the atomicity fixture initializes its task database");
        store
            .connection
            .execute_batch(trigger_sql)
            .expect("the fixture injects a deterministic SQLite failure");
        drop(store);

        init_from_manifest(
            atomic_workspace.root(),
            Path::new("docs/milestones/001-preflight.md"),
            false,
        )
        .expect_err("an injected persistence failure aborts milestone initialization");

        let connection = rusqlite::Connection::open(&database)
            .expect("the failed preflight leaves an inspectable task database");
        for table in ["tasks", "milestone_contract_matrix", "task_blackboard"] {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("the atomicity fixture can count persisted rows");
            assert_eq!(count, 0, "failed preflight must roll back {table}");
        }
        assert!(
            !atomic_workspace
                .root()
                .join(".worktrees/preflight-fixture")
                .exists(),
            "failed preflight removes the primary worktree"
        );
        assert!(
            !atomic_secondary
                .join(".worktrees/preflight-fixture")
                .exists(),
            "failed preflight removes the secondary worktree"
        );
    };

    assert_atomic_failure(
        "preflight_matrix_write_failure",
        "CREATE TRIGGER reject_contract_matrix BEFORE INSERT ON milestone_contract_matrix \
         BEGIN SELECT RAISE(ABORT, 'injected matrix write failure'); END;",
    );
    assert_atomic_failure(
        "preflight_blackboard_write_failure",
        "CREATE TRIGGER reject_initial_blackboard BEFORE INSERT ON task_blackboard \
         WHEN NEW.author='milestone-init' BEGIN SELECT RAISE(ABORT, 'injected blackboard write failure'); END;",
    );
}
