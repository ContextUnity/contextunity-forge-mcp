use super::*;

#[test]
fn milestone_clear_prunes_tasks_traces_metadata_and_git_state_refs() {
    let harness = MilestoneHarness::linear("m-close");
    let tasks = {
        let store = harness.store();
        store.list(None, "all", None).unwrap()
    };
    assert!(!tasks.is_empty());

    // Create a git state checkpoint ref
    let root = harness.root();
    let head = String::from_utf8(
        crate::acdd::support::git_command(root, &["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();
    let state_ref = "refs/forge/state/forge-mcp/forge-mcp/m-close/finish";
    crate::acdd::support::git_command(root, &["update-ref", state_ref, head])
        .status()
        .unwrap();
    assert!(
        crate::acdd::support::git_command(root, &["rev-parse", "--verify", state_ref])
            .status()
            .unwrap()
            .success()
    );

    // Run milestone clear via CLI
    let cleared = harness.run_cli(&["milestone", "clear", "010"]).unwrap();
    assert_eq!(cleared["id"], "m-close");
    assert_eq!(cleared["cleared"], true);

    // Verify tasks are purged from SQLite
    let tasks_after = {
        let store = harness.store();
        store.list(None, "all", None).unwrap()
    };
    assert_eq!(tasks_after.len(), 0);

    // Verify git state ref is deleted
    assert!(
        !crate::acdd::support::git_command(root, &["rev-parse", "--verify", state_ref])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn plan_clear_and_archive_commands() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "docs/plans/cleanup-test.md",
        "---\ntitle: Cleanup Test Plan\ndoc_type: plan\npurpose: testing cleanup\ndepends_on: []\n---\n# Cleanup Test Plan\n\n## 1. Step One\n- **Target**: Target one\n- **Scope**: [src]\n- **Status**: ready\n",
    );

    let root = harness.root();
    let head = String::from_utf8(
        crate::acdd::support::git_command(root, &["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();
    let worktree = root.canonicalize().unwrap();
    let namespace = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(worktree.to_string_lossy().as_bytes())
    );
    let plan_tag = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(b"docs/plans/cleanup-test.md")
    );
    let snapshot_ref = format!(
        "refs/forge/snapshots/{namespace}/plan-{}-step-1",
        &plan_tag[..16]
    );
    crate::acdd::support::git_command(root, &["update-ref", &snapshot_ref, head])
        .status()
        .unwrap();
    assert!(
        crate::acdd::support::git_command(root, &["rev-parse", "--verify", &snapshot_ref])
            .status()
            .unwrap()
            .success()
    );

    // Run plan clear
    let cleared = harness.run_cli(&["plan", "clear", "cleanup-test"]).unwrap();
    assert_eq!(cleared["cleared"], true);
    // Manifest still exists
    assert!(root.join("docs/plans/cleanup-test.md").exists());
    // Snapshot ref is deleted
    assert!(
        !crate::acdd::support::git_command(root, &["rev-parse", "--verify", &snapshot_ref])
            .status()
            .unwrap()
            .success()
    );

    // Re-create snapshot ref to test archive
    crate::acdd::support::git_command(root, &["update-ref", &snapshot_ref, head])
        .status()
        .unwrap();

    // Run plan archive
    let archived = harness
        .run_cli(&["plan", "archive", "cleanup-test"])
        .unwrap();
    assert_eq!(archived["archived"], true);
    assert!(!root.join("docs/plans/cleanup-test.md").exists());
    assert!(root.join("docs/plans/archive/cleanup-test.md").exists());
    assert!(
        !crate::acdd::support::git_command(root, &["rev-parse", "--verify", &snapshot_ref])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn workflow_clear_command() {
    let harness = MilestoneHarness::configured_workspace();
    let runs_dir = harness.root().join(".forge/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let log_file = runs_dir.join("sample-workflow-2026-11-10.log");
    std::fs::write(&log_file, "workflow run log output").unwrap();
    assert!(log_file.exists());

    let cleared = harness
        .run_cli(&["workflow", "clear", "sample-workflow"])
        .unwrap();
    assert_eq!(cleared["cleared"], true);
    assert!(!log_file.exists());
}

#[test]
fn universal_clear_command_dispatch() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "docs/plans/uni-plan.md",
        "---\ntitle: Uni Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Uni Plan\n\n## 1. Step One\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    let cleared = harness.run_cli(&["clear", "plan", "uni-plan"]).unwrap();
    assert_eq!(cleared["cleared"], true);
}

#[test]
fn milestone_clear_preserves_neighbors_foreign_tasks_and_wildcards() {
    let harness = MilestoneHarness::linear("m-close");
    let root = harness.root();
    let head = String::from_utf8(
        crate::acdd::support::git_command(root, &["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();

    // Create neighbor refs that must NOT be deleted
    let neighbor_ref1 = "refs/forge/state/forge-mcp/forge-mcp/m-close-other/first";
    let neighbor_ref2 = "refs/forge/state/foreign/project/m-close/first";
    crate::acdd::support::git_command(root, &["update-ref", neighbor_ref1, head])
        .status()
        .unwrap();
    crate::acdd::support::git_command(root, &["update-ref", neighbor_ref2, head])
        .status()
        .unwrap();

    // Seed execution provenance and foreign tasks into SQLite
    let db_path = root.join(".forge/tasks.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        // Foreign task sharing milestone_ref
        conn.execute(
            "INSERT INTO tasks(task_id,milestone_ref,descriptor) VALUES(?1,?2,?3)",
            rusqlite::params![
                "foreign/m-other:neighbor",
                "docs/milestones/010-close.md",
                "{\"task_id\":\"foreign/m-other:neighbor\",\"project\":\"foreign\",\"milestone_id\":\"m-other\"}"
            ],
        ).unwrap();

        // Foreign trace with underscore / wildcard name
        conn.execute(
            "INSERT INTO gate_execution_traces(milestone_id,subject,gate_path,command_id,command_str,status,duration_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params!["foreign/project/mXa", "forge-mcp/mXa:first", "build", "test", "true", "passed", 1],
        ).unwrap();

        // Provenance records for m-close
        let _ = conn.execute(
            "INSERT INTO gate_execution_runs(milestone_id,subject,gate_path,claim_revision,contract_revision,worker_id,worktree,candidate_fingerprint,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params!["forge-mcp/forge-mcp/m-close", "forge-mcp/m-close:first", "build", 1, 1, "builder", root.to_str().unwrap(), "candidate", "completed"],
        ).unwrap();
        let run_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO gate_execution_attempts(run_id,milestone_id,subject,gate_path,command_id,attempt_id,state) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![run_id, "forge-mcp/forge-mcp/m-close", "forge-mcp/m-close:first", "build", "check", 1, "completed"],
        ).unwrap();
        conn.execute(
            "INSERT INTO command_execution_proofs(run_id,command_id,attempt_id,payload) VALUES(?1,?2,?3,?4)",
            rusqlite::params![run_id, "check", 1, "{}"],
        ).unwrap();
    }

    // Run clear on m-close
    let cleared = harness.run_cli(&["milestone", "clear", "m-close"]).unwrap();
    assert_eq!(cleared["cleared"], true);

    // Neighbor refs must still exist
    assert!(
        crate::acdd::support::git_command(root, &["rev-parse", "--verify", neighbor_ref1])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::acdd::support::git_command(root, &["rev-parse", "--verify", neighbor_ref2])
            .status()
            .unwrap()
            .success()
    );

    // Verify foreign task and wildcard trace survive, while provenance is pruned
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let foreign_tasks: i64 = conn
            .query_row(
                "SELECT count(*) FROM tasks WHERE task_id='foreign/m-other:neighbor'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(foreign_tasks, 1);

        let wildcard_traces: i64 = conn
            .query_row(
                "SELECT count(*) FROM gate_execution_traces WHERE subject='forge-mcp/mXa:first'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(wildcard_traces, 1);

        let provenance_runs: i64 = conn
            .query_row(
                "SELECT count(*) FROM gate_execution_runs WHERE milestone_id='forge-mcp/forge-mcp/m-close'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provenance_runs, 0);

        let provenance_proofs: i64 = conn
            .query_row("SELECT count(*) FROM command_execution_proofs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(provenance_proofs, 0);
    }
}

#[test]
fn milestone_clear_rejects_pending_publication_fence() {
    let harness = MilestoneHarness::linear("m-close");
    let db_path = harness.root().join(".forge/tasks.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO task_store_metadata(key,value) VALUES('active_checkpoint_pending',?1)",
            ["{\"active\":[],\"retired\":[]}"],
        )
        .unwrap();
    }
    let res = harness.run_cli(&["milestone", "clear", "m-close"]);
    assert!(
        res.is_err(),
        "expected rejection when active_checkpoint_pending is present"
    );
}

#[test]
fn milestone_ambiguous_selector_fails_closed() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "docs/milestones/010-a.md",
        "---\nid: m-a\ntitle: Milestone A\nstatus: active\n---\n# M A\n## Tasks in this milestone\n",
    );
    harness.write(
        "docs/milestones/010-b.md",
        "---\nid: m-b\ntitle: Milestone B\nstatus: active\n---\n# M B\n## Tasks in this milestone\n",
    );
    let res = harness.run_cli(&["milestone", "clear", "010"]);
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("MILESTONE_AMBIGUOUS"));
}

#[test]
fn plan_archive_rejects_overwrite_and_symlinks() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    harness.write(
        "docs/plans/cleanup-test.md",
        "---\ntitle: Cleanup Test Plan\ndoc_type: plan\npurpose: testing cleanup\ndepends_on: []\n---\n# Cleanup Test Plan\n\n## 1. Step One\n- **Target**: Target one\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    harness.write(
        "docs/plans/archive/cleanup-test.md",
        "previous archived authority\n",
    );

    // Attempting to archive when destination already exists must fail closed
    let res = harness.run_cli(&["plan", "archive", "cleanup-test"]);
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("PLAN_ARCHIVE_EXISTS"));
    assert_eq!(
        std::fs::read_to_string(root.join("docs/plans/archive/cleanup-test.md")).unwrap(),
        "previous archived authority\n"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside_dir = root.join("outside");
        std::fs::create_dir_all(&outside_dir).unwrap();

        let symlink_harness = MilestoneHarness::configured_workspace();
        let sl_root = symlink_harness.root();
        symlink_harness.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\nplans: [plans]\ntasks_db: .forge/tasks.sqlite\n",
        );
        symlink_harness.write(
            "plans/symlink-plan.md",
            "---\ntitle: S\ndoc_type: plan\npurpose: s\ndepends_on: []\n---\n# S\n## 1. S\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
        );
        std::fs::create_dir_all(sl_root.join("docs/plans")).unwrap();
        symlink(&outside_dir, sl_root.join("docs/plans/archive")).unwrap();

        let res = symlink_harness.run_cli(&["plan", "archive", "symlink-plan"]);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("PLAN_PATH_INVALID"));

        let wf_harness = MilestoneHarness::configured_workspace();
        let wf_root = wf_harness.root();
        std::fs::create_dir_all(wf_root.join(".forge")).unwrap();
        symlink(&outside_dir, wf_root.join(".forge/runs")).unwrap();
        let res_wf = wf_harness.run_cli(&["workflow", "clear", "check"]);
        assert!(res_wf.is_err());
        assert!(res_wf
            .unwrap_err()
            .to_string()
            .contains("WORKFLOW_PATH_INVALID"));
    }
}

#[test]
fn workflow_clear_preserves_neighbor_logs() {
    let harness = MilestoneHarness::configured_workspace();
    let runs_dir = harness.root().join(".forge/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let neighbor_log = runs_dir.join("check-other.log");
    std::fs::write(&neighbor_log, "neighbor log").unwrap();

    let target_log = runs_dir.join("check-2026-11-10.log");
    std::fs::write(&target_log, "target log").unwrap();

    let cleared = harness.run_cli(&["workflow", "clear", "check"]).unwrap();
    assert_eq!(cleared["cleared"], true);
    assert!(!target_log.exists());
    assert!(neighbor_log.exists());
}

#[test]
fn workflow_ambiguity_fails_closed() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "docs/workflows/check.yaml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    harness.write(
        "docs/workflows/check.yml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    let runs_dir = harness.root().join(".forge/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let log_path = runs_dir.join("check-2026-11-10.log");
    std::fs::write(&log_path, "target log\n").unwrap();

    let res = harness.run_cli(&["workflow", "clear", "check"]);
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("WORKFLOW_AMBIGUOUS"));
    assert!(log_path.exists());
}

#[test]
fn workflow_clear_qualified_selector_shared_stem_fails_closed() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "docs/workflows/check.yaml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    harness.write(
        "docs/workflows/check.yml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    let runs_dir = harness.root().join(".forge/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let log_exact = runs_dir.join("check.log");
    let log_ts = runs_dir.join("check-2026-11-10.log");
    std::fs::write(&log_exact, "exact log\n").unwrap();
    std::fs::write(&log_ts, "timestamp log\n").unwrap();

    // 1. Qualified selector on 'workflow clear' fails closed due to ambiguous ownership
    let res = harness.run_cli(&["workflow", "clear", "docs/workflows/check.yaml"]);
    assert!(
        res.is_err(),
        "qualified selector must fail when another workflow shares stem"
    );
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("WORKFLOW_AMBIGUOUS"),
        "expected WORKFLOW_AMBIGUOUS, got: {err}"
    );
    assert!(
        log_exact.exists(),
        "exact log must be preserved on ambiguous ownership"
    );
    assert!(
        log_ts.exists(),
        "timestamp log must be preserved on ambiguous ownership"
    );

    // 2. Qualified selector on universal 'clear workflow' also fails closed
    let res2 = harness.run_cli(&["clear", "workflow", "docs/workflows/check.yaml"]);
    assert!(
        res2.is_err(),
        "universal clear workflow must fail when another workflow shares stem"
    );
    let err2 = res2.unwrap_err().to_string();
    assert!(
        err2.contains("WORKFLOW_AMBIGUOUS"),
        "expected WORKFLOW_AMBIGUOUS, got: {err2}"
    );
    assert!(
        log_exact.exists(),
        "exact log must be preserved on ambiguous ownership"
    );
    assert!(
        log_ts.exists(),
        "timestamp log must be preserved on ambiguous ownership"
    );
}

#[test]
fn workflow_clear_shared_stem_trace_only_and_topologies_fails_closed() {
    // 1. Trace-only with NO .forge/runs directory
    let harness1 = MilestoneHarness::configured_workspace();
    let root1 = harness1.root();
    harness1.write(
        "docs/workflows/check.yaml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    harness1.write(
        "docs/workflows/check.yml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    let db_path1 = root1.join(".forge/tasks.sqlite");
    std::fs::create_dir_all(db_path1.parent().unwrap()).unwrap();
    let conn1 = rusqlite::Connection::open(&db_path1).unwrap();
    conn1
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS gate_execution_traces (
            milestone_id TEXT,
            subject TEXT,
            gate_path TEXT,
            command_id TEXT,
            command_str TEXT,
            status TEXT,
            duration_ms INTEGER
        );",
        )
        .unwrap();
    conn1.execute(
        "INSERT INTO gate_execution_traces(milestone_id,subject,gate_path,command_id,command_str,status,duration_ms) VALUES(?,?,?,?,?,?,?)",
        rusqlite::params!["workflow/check", "check#gate", "gate", "own", "echo check", "passed", 1],
    ).unwrap();
    assert!(!root1.join(".forge/runs").exists());

    let res1 = harness1.run_cli(&["workflow", "clear", "docs/workflows/check.yaml"]);
    assert!(res1.is_err());
    assert!(res1.unwrap_err().to_string().contains("WORKFLOW_AMBIGUOUS"));
    let count1: i64 = conn1
        .query_row(
            "SELECT count(*) FROM gate_execution_traces WHERE subject='check#gate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count1, 1,
        "trace must be preserved when runs dir is missing"
    );

    // 2. Universal trace-only
    let res1_uni = harness1.run_cli(&["clear", "workflow", "docs/workflows/check.yaml"]);
    assert!(res1_uni.is_err());
    assert!(res1_uni
        .unwrap_err()
        .to_string()
        .contains("WORKFLOW_AMBIGUOUS"));
    let count1_uni: i64 = conn1
        .query_row(
            "SELECT count(*) FROM gate_execution_traces WHERE subject='check#gate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count1_uni, 1, "trace must be preserved on universal clear");

    // 3. Empty runs directory
    let runs_dir1 = root1.join(".forge/runs");
    std::fs::create_dir_all(&runs_dir1).unwrap();
    let res1_empty = harness1.run_cli(&["workflow", "clear", "docs/workflows/check.yaml"]);
    assert!(res1_empty.is_err());
    assert!(res1_empty
        .unwrap_err()
        .to_string()
        .contains("WORKFLOW_AMBIGUOUS"));
    let count1_empty: i64 = conn1
        .query_row(
            "SELECT count(*) FROM gate_execution_traces WHERE subject='check#gate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count1_empty, 1,
        "trace must be preserved with empty runs dir"
    );

    // 4. Runs directory with unrelated logs only
    std::fs::write(runs_dir1.join("other.log"), "unrelated log\n").unwrap();
    let res1_unrelated = harness1.run_cli(&["workflow", "clear", "docs/workflows/check.yaml"]);
    assert!(res1_unrelated.is_err());
    assert!(res1_unrelated
        .unwrap_err()
        .to_string()
        .contains("WORKFLOW_AMBIGUOUS"));
    assert!(
        runs_dir1.join("other.log").exists(),
        "unrelated log must be preserved"
    );
    let count1_unrelated: i64 = conn1
        .query_row(
            "SELECT count(*) FROM gate_execution_traces WHERE subject='check#gate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count1_unrelated, 1,
        "trace must be preserved with unrelated log"
    );

    // 5. Unique workflow control: clearing succeeds and deletes trace
    let harness2 = MilestoneHarness::configured_workspace();
    let root2 = harness2.root();
    harness2.write(
        "docs/workflows/check.yaml",
        "version: 2\ninherit: none\ngates: []\n",
    );
    let db_path2 = root2.join(".forge/tasks.sqlite");
    std::fs::create_dir_all(db_path2.parent().unwrap()).unwrap();
    let conn2 = rusqlite::Connection::open(&db_path2).unwrap();
    conn2
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS gate_execution_traces (
            milestone_id TEXT,
            subject TEXT,
            gate_path TEXT,
            command_id TEXT,
            command_str TEXT,
            status TEXT,
            duration_ms INTEGER
        );",
        )
        .unwrap();
    conn2.execute(
        "INSERT INTO gate_execution_traces(milestone_id,subject,gate_path,command_id,command_str,status,duration_ms) VALUES(?,?,?,?,?,?,?)",
        rusqlite::params!["workflow/check", "check#gate", "gate", "own", "echo check", "passed", 1],
    ).unwrap();
    let res2 = harness2
        .run_cli(&["workflow", "clear", "docs/workflows/check.yaml"])
        .unwrap();
    assert_eq!(res2["cleared"], true);
    assert_eq!(res2["traces_deleted"], 1);
    let count2: i64 = conn2
        .query_row(
            "SELECT count(*) FROM gate_execution_traces WHERE subject='check#gate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count2, 0, "trace must be deleted for unique workflow");
}

#[test]
fn plan_prefix_isolation_and_filename_compatibility() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    harness.write(
        "docs/plans/foo.md",
        "---\ntitle: Foo Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Foo Plan\n\n## 1. Step One\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    harness.write(
        "docs/plans/foo-bar.md",
        "---\ntitle: Foo Bar Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Foo Bar Plan\n\n## 1. Step One\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    harness.write(
        "docs/plans/release plan.md",
        "---\ntitle: Release Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Release Plan\n\n## 1. Step One\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );

    // Create distinct snapshot refs using exact plan tag hashing
    let head = String::from_utf8(
        crate::acdd::support::git_command(root, &["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();

    let worktree = root.canonicalize().unwrap();
    let namespace = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(worktree.to_string_lossy().as_bytes())
    );
    let foo_tag = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(b"docs/plans/foo.md")
    );
    let foo_bar_tag = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(b"docs/plans/foo-bar.md")
    );

    let foo_ref = format!(
        "refs/forge/snapshots/{namespace}/plan-{}-commit1",
        &foo_tag[..16]
    );
    let foo_bar_ref = format!(
        "refs/forge/snapshots/{namespace}/plan-{}-commit1",
        &foo_bar_tag[..16]
    );

    crate::acdd::support::git_command(root, &["update-ref", &foo_ref, head])
        .status()
        .unwrap();
    crate::acdd::support::git_command(root, &["update-ref", &foo_bar_ref, head])
        .status()
        .unwrap();

    let cleared = harness.run_cli(&["plan", "clear", "foo"]).unwrap();
    assert_eq!(cleared["cleared"], true);

    // foo_ref deleted, foo_bar_ref preserved!
    assert!(
        !crate::acdd::support::git_command(root, &["rev-parse", "--verify", &foo_ref])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::acdd::support::git_command(root, &["rev-parse", "--verify", &foo_bar_ref])
            .status()
            .unwrap()
            .success()
    );

    // Release plan with spaces in filename is clearable
    let cleared_release = harness.run_cli(&["plan", "clear", "release plan"]).unwrap();
    assert_eq!(cleared_release["cleared"], true);
}

#[test]
fn milestone_enumeration_failure_fails_closed() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    let head = String::from_utf8(
        crate::acdd::support::git_command(root, &["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();
    let state_ref = "refs/forge/state/forge-mcp/forge-mcp/m-close/first";
    crate::acdd::support::git_command(root, &["update-ref", state_ref, head])
        .status()
        .unwrap();
    std::fs::write(root.join(".git/packed-refs"), "invalid packed refs\n").unwrap();
    let res = harness.run_cli(&["milestone", "clear", "m-close"]);
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("MILESTONE_CLEAR_FAILED") || err.contains("git for-each-ref failed"));
}

#[test]
fn workflow_and_plan_trace_ownership_preserves_milestone_verification_traces() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    let db_path = root.join(".forge/tasks.sqlite");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gate_execution_traces (
            milestone_id TEXT,
            subject TEXT,
            gate_path TEXT,
            command_id TEXT,
            command_str TEXT,
            status TEXT,
            duration_ms INTEGER
        );
        INSERT INTO gate_execution_traces(milestone_id, subject, gate_path, command_id, command_str, status, duration_ms)
        VALUES('foreign/project/m-important', 'milestone', 'verify', 'check', 'true', 'passed', 1);",
    ).unwrap();

    // Run workflow clear milestone
    let wf_cleared = harness
        .run_cli(&["workflow", "clear", "milestone"])
        .unwrap();
    assert_eq!(wf_cleared["traces_deleted"], 0);

    // Verify row remains
    let count: i64 = conn
        .query_row("SELECT count(*) FROM gate_execution_traces", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);

    // Write a plan named milestone.md
    harness.write(
        "docs/plans/milestone.md",
        "---\ntitle: Milestone Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Milestone Plan\n## 1. S\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    let plan_cleared = harness.run_cli(&["plan", "clear", "milestone"]).unwrap();
    assert_eq!(plan_cleared["traces_deleted"], 0);

    // Row still remains!
    let count: i64 = conn
        .query_row("SELECT count(*) FROM gate_execution_traces", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn numeric_workflow_neighbor_is_preserved() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    for name in &["check", "check-2", "check-2026", "check-2026-11-10"] {
        harness.write(
            format!("docs/workflows/{name}.yaml"),
            "version: 2\ninherit: none\ngates: []\n",
        );
    }
    let runs_dir = root.join(".forge/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let check_log = runs_dir.join("check-2026-11-12.log");
    let check_exact_neighbor_log = runs_dir.join("check-2026-11-10.log");
    let check2_log = runs_dir.join("check-2-2026-11-10.log");
    let check2026_log = runs_dir.join("check-2026-2026-11-10.log");
    let check_date_log = runs_dir.join("check-2026-11-10-2026-11-10.log");
    let unicode_log = runs_dir.join("check-202é.log");

    std::fs::write(&check_log, "target workflow log\n").unwrap();
    std::fs::write(&check_exact_neighbor_log, "neighbor workflow exact log\n").unwrap();
    std::fs::write(&check2_log, "neighbor workflow log 2\n").unwrap();
    std::fs::write(&check2026_log, "neighbor workflow log 2026\n").unwrap();
    std::fs::write(&check_date_log, "neighbor workflow log date\n").unwrap();
    std::fs::write(&unicode_log, "unrelated unicode log\n").unwrap();

    let cleared = harness.run_cli(&["workflow", "clear", "check"]).unwrap();
    assert_eq!(cleared["cleared"], true);
    assert_eq!(cleared["files_deleted"], 1);
    assert!(!check_log.exists());
    assert!(check_exact_neighbor_log.exists());
    assert!(check2_log.exists());
    assert!(check2026_log.exists());
    assert!(check_date_log.exists());
    assert!(unicode_log.exists());
}

#[cfg(unix)]
#[test]
fn dangling_workflow_runs_fails_closed() {
    use std::os::unix::fs::symlink;
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    std::fs::create_dir_all(root.join(".forge")).unwrap();
    symlink(root.join("missing-runs"), root.join(".forge/runs")).unwrap();
    let res = harness.run_cli(&["workflow", "clear", "check"]);
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("WORKFLOW_PATH_INVALID"));
}

#[test]
fn invalid_task_identity_configuration_fails_closed() {
    let harness = MilestoneHarness::configured_workspace();
    harness.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: ../external-tasks.sqlite\n",
    );
    let res_wf = harness.run_cli(&["workflow", "clear", "check"]);
    assert!(res_wf.is_err());

    harness.write(
        "docs/plans/shared.md",
        "---\ntitle: Shared Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Shared Plan\n## 1. S\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    let res_plan = harness.run_cli(&["plan", "clear", "shared"]);
    assert!(res_plan.is_err());
}

#[cfg(unix)]
#[test]
fn db_confinement_fails_closed() {
    use std::os::unix::fs::symlink;
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    let outside_dir = root.parent().unwrap().join("external-test-db");
    std::fs::create_dir_all(&outside_dir).unwrap();
    symlink(&outside_dir, root.join("link")).unwrap();

    // Absolute symlink parent
    let abs_symlink_db = root.join("link/tasks.sqlite");
    harness.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [src]\ndocs: [docs]\ntasks_db: {}\n",
            abs_symlink_db.display()
        ),
    );
    let res = harness.run_cli(&["milestone", "clear", "m-init"]);
    assert!(res.is_err());
    assert!(!outside_dir.join("tasks.sqlite").exists());

    // Absolute traversal with '..'
    let abs_dotdot_db = root.join("../external-test-db/tasks.sqlite");
    harness.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [src]\ndocs: [docs]\ntasks_db: {}\n",
            abs_dotdot_db.display()
        ),
    );
    let res_dotdot = harness.run_cli(&["milestone", "clear", "m-init"]);
    assert!(res_dotdot.is_err());
    assert!(!outside_dir.join("tasks.sqlite").exists());
}

#[test]
fn named_task_snapshot_ownership_is_preserved() {
    let harness = MilestoneHarness::configured_workspace();
    let root = harness.root();
    harness.write(
        "docs/plans/shared.md",
        "---\ntitle: Shared Plan\ndoc_type: plan\npurpose: testing\ndepends_on: []\n---\n# Shared Plan\n## 1. S\n- **Target**: T\n- **Scope**: [src]\n- **Status**: ready\n",
    );
    harness.write(
        "profile.yaml",
        &std::fs::read_to_string("tests/acdd/fixtures/plans/snapshot.yaml").unwrap(),
    );

    // Create real named snapshot using actual engine
    forge_tasks::manage(
        root,
        forge_tasks::Manage {
            action: forge_tasks::ManageAction::Snapshot,
            name: Some("shared".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let worktree = root.canonicalize().unwrap();
    let namespace = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(worktree.to_string_lossy().as_bytes())
    );
    let task_snapshot_ref = format!("refs/forge/snapshots/{namespace}/shared");
    assert!(crate::acdd::support::git_command(
        root,
        &["rev-parse", "--verify", &task_snapshot_ref]
    )
    .status()
    .unwrap()
    .success());

    // Run real plan to generate plan snapshot
    let plan_run = harness
        .run_cli(&[
            "--profile",
            "profile.yaml",
            "plan",
            "run",
            "shared",
            "--in-place",
        ])
        .unwrap();
    assert_eq!(plan_run["status"], "completed");

    let cleared = harness.run_cli(&["plan", "clear", "shared"]).unwrap();
    assert_eq!(cleared["cleared"], true);
    assert_eq!(cleared["refs_pruned"], 1);

    // Task snapshot is strictly preserved!
    assert!(crate::acdd::support::git_command(
        root,
        &["rev-parse", "--verify", &task_snapshot_ref]
    )
    .status()
    .unwrap()
    .success());
}
