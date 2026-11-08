use super::support::*;

const MANIFEST: &str = "docs/milestones/010-scope-sync.md";
const TASK_ID: &str = "forge-mcp/m-scope-sync:owner";

fn scope_fixture() -> Workspace {
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("src/entry.rs", "pub fn entry() {}\n");
    root.write("tests/admitted.rs", "// admitted test source\n");
    root.write(
        MANIFEST,
        "---\nid: m-scope-sync\ntitle: Scope Sync\ndoc_type: contract\nstatus: active\n---\n# Scope Sync\n```yaml\ntask_ref: owner\ntarget: Exercise atomic scope extension\nproof_policy: seam-test-first\nscope: [src/entry.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    tasks::manage(
        root.root(),
        serde_json::from_value(json!({"action":"sync","milestone_ref":MANIFEST})).unwrap(),
    )
    .unwrap();
    root
}

fn extend_scope(path: &str) -> tasks::Manage {
    serde_json::from_value(json!({
        "action": "extend_scope",
        "task_id": TASK_ID,
        "paths": [path],
    }))
    .unwrap()
}

fn stored_task(root: &Workspace) -> TasksStore {
    tasks::store(root.root()).unwrap()
}

#[test]
fn scope_extension_updates_sqlite_and_milestone_manifest() {
    let root = scope_fixture();
    let before = stored_task(&root).inspect(TASK_ID).unwrap();
    let original_contract_revision = before.contract_revision;

    tasks::manage(root.root(), extend_scope("tests/admitted.rs")).unwrap();

    let manifest_text = std::fs::read_to_string(root.root().join(MANIFEST)).unwrap();
    let milestone = Milestone::parse(&manifest_text, "forge-mcp").unwrap();
    let manifest_task = &milestone.tasks[0];
    let extended = stored_task(&root).inspect(TASK_ID).unwrap();
    assert!(extended
        .spec
        .scope
        .iter()
        .any(|path| path == "tests/admitted.rs"));
    assert_eq!(extended.spec.extended_scope, ["tests/admitted.rs"]);
    assert_eq!(extended.initial_scope, ["src/entry.rs"]);
    assert!(manifest_task
        .scope
        .iter()
        .any(|path| path == "tests/admitted.rs"));
    assert_eq!(manifest_task.extended_scope, ["tests/admitted.rs"]);
    assert_eq!(
        extended.digest,
        milestone.digest(manifest_task).unwrap(),
        "stored authority digest must match the synchronized manifest"
    );
    assert_eq!(extended.contract_revision, original_contract_revision);

    tasks::manage(
        root.root(),
        serde_json::from_value(json!({"action":"sync","milestone_ref":MANIFEST})).unwrap(),
    )
    .unwrap();
    let after_sync = stored_task(&root).inspect(TASK_ID).unwrap();
    assert_eq!(after_sync.spec.scope, extended.spec.scope);
    assert_eq!(after_sync.spec.extended_scope, extended.spec.extended_scope);
    assert_eq!(after_sync.digest, extended.digest);
    assert_eq!(after_sync.contract_revision, original_contract_revision);
}

#[test]
fn scope_extension_rejects_invalid_manifest_without_persisting_sqlite_state() {
    let root = scope_fixture();
    let before = stored_task(&root).inspect(TASK_ID).unwrap();
    let malformed_manifest = std::fs::read_to_string(root.root().join(MANIFEST))
        .unwrap()
        .replace(
            "target: Exercise atomic scope extension",
            "target: [invalid yaml",
        );
    assert_ne!(
        malformed_manifest,
        std::fs::read_to_string(root.root().join(MANIFEST)).unwrap()
    );
    root.write(MANIFEST, &malformed_manifest);

    assert!(tasks::manage(root.root(), extend_scope("tests/admitted.rs")).is_err());

    assert_eq!(
        std::fs::read_to_string(root.root().join(MANIFEST)).unwrap(),
        malformed_manifest
    );
    let after = stored_task(&root).inspect(TASK_ID).unwrap();
    assert_eq!(after.spec.scope, before.spec.scope);
    assert_eq!(after.spec.extended_scope, before.spec.extended_scope);
    assert_eq!(after.digest, before.digest);
    assert_eq!(after.contract_revision, before.contract_revision);
}

#[test]
fn scope_extension_keeps_both_projections_unchanged_when_sqlite_writer_is_blocked() {
    let root = scope_fixture();
    let before = stored_task(&root).inspect(TASK_ID).unwrap();
    let original_manifest = std::fs::read_to_string(root.root().join(MANIFEST)).unwrap();
    let lock = rusqlite::Connection::open(tasks::database_path(root.root()).unwrap()).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    let result = tasks::manage(root.root(), extend_scope("tests/admitted.rs"));
    lock.execute_batch("ROLLBACK").unwrap();
    assert!(
        result.is_err(),
        "an active SQLite writer must reject extension"
    );

    assert_eq!(
        std::fs::read_to_string(root.root().join(MANIFEST)).unwrap(),
        original_manifest
    );
    let after = stored_task(&root).inspect(TASK_ID).unwrap();
    assert_eq!(after.spec.scope, before.spec.scope);
    assert_eq!(after.spec.extended_scope, before.spec.extended_scope);
    assert_eq!(after.digest, before.digest);
    assert_eq!(after.contract_revision, before.contract_revision);
}
