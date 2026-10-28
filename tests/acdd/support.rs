pub(crate) use crate::common::Workspace;
pub(crate) use contextunity_forge_mcp::{
    core::tasks::{gates::Evidence, Milestone, Receipt, ReceiptRollup, ReviewSummary},
    db::{
        tasks_store::{TasksStore, RETENTION_SECONDS},
        writer,
    },
    engine::tasks,
};
pub(crate) use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const GATES: [&str; 4] = ["contract", "build", "review", "deliver"];

pub(crate) fn fixture_integration_evidence(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> contextunity_forge_mcp::engine::milestones::IntegrationEvidence {
    use contextunity_forge_mcp::engine::{milestones, tasks::TaskSettings};
    let exclude = root.join(
        contextunity_forge_mcp::core::git::git_stdout(
            root,
            &["rev-parse", "--git-path", "info/exclude"],
            "fixture Git directory",
        )
        .unwrap(),
    );
    let mut ignored = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !ignored.lines().any(|line| line == ".forge/") {
        ignored.push_str("\n.forge/\n");
    }
    std::fs::write(exclude, ignored).unwrap();
    let settings: TaskSettings = serde_yaml::from_str(
        &std::fs::read_to_string(root.join("forge-mcp.yaml")).unwrap_or_default(),
    )
    .unwrap();
    let view = milestones::show(root, selector, false).unwrap();
    let repository = view["frontmatter"]["repository"]
        .as_str()
        .unwrap_or(&settings.task_repository);
    let listed = tasks::list(
        root,
        tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some(selector.into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .ok();
    let canonical_project = listed
        .as_ref()
        .and_then(|listed| listed["tasks"][0]["task_id"].as_str())
        .and_then(|id| id.split_once('/').map(|(project, _)| project));
    let declared_project = view["frontmatter"]["project"]
        .as_str()
        .or(settings.task_project.as_deref())
        .unwrap_or(repository);
    let project = canonical_project.unwrap_or_else(|| {
        settings
            .aliases
            .get(declared_project)
            .map(String::as_str)
            .unwrap_or(declared_project)
    });
    let store = TasksStore::open_project(&tasks::database_path(root).unwrap(), repository, project)
        .unwrap();
    let prefix = format!("{project}/{}:", view["id"].as_str().unwrap());
    let mut candidates = store.list(None, "all", None).unwrap();
    candidates.retain(|task| task.task_id.starts_with(&prefix));
    candidates.sort_by_key(|task| (task.wave_index, task.task_id.clone()));
    let target = candidates
        .first()
        .and_then(|task| {
            let details = store.inspect_details(&task.task_id).unwrap();
            details["gates"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|gate| {
                    if gate["stage"] != "build" {
                        return None;
                    }
                    let evidence: Value = serde_json::from_str(gate["evidence"].as_str()?).ok()?;
                    evidence["candidate_baseline_head"]
                        .as_str()
                        .map(str::to_owned)
                })
        })
        .unwrap_or_else(|| git_head(root));
    // Malformed/incomplete fixtures still reach the actual handoff rejection path.
    let binding = milestones::handoff_context(root, selector, &target).map(|context| context["binding"].clone()).unwrap_or_else(|_| json!({
        "version":1,"repository":repository,"project":project,"milestone":view["id"],"authority_digest":"unavailable","implementation_digest":"unavailable","target_base_digest":"unavailable","builders":["builder"]
    }));
    serde_json::from_value(json!({
        "binding":binding,"target_base":target,
        "review":{"worker_id":"integration-reviewer","decision":"pass","summary":"Fixture independently reviews the combined delta against admitted tasks","scope_findings":[],"contract_findings":[]},
        "verification":{"command":command,"exit_code":if failed == 0 {0} else {1},"tests_passed":passed,"tests_failed":failed}
    })).unwrap()
}

pub(crate) fn handoff_with_fixture_evidence(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    command: &str,
    passed: u64,
    failed: u64,
) -> anyhow::Result<Value> {
    contextunity_forge_mcp::engine::milestones::handoff(
        root,
        selector,
        commit,
        &fixture_integration_evidence(root, selector, command, passed, failed),
    )
}

pub(crate) fn fixture_evidence_file(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> String {
    let path = root.join(".forge/integration-evidence.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&fixture_integration_evidence(
            root, selector, command, passed, failed,
        ))
        .unwrap(),
    )
    .unwrap();
    path.to_string_lossy().into_owned()
}

pub(crate) fn commit_handoff_fixture_inputs(root: &ScopedWorkspace, paths: &[&str]) {
    let added = std::process::Command::new("git")
        .args(["add", "--"])
        .args(paths)
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(added.status.success(), "{added:?}");
    let committed = std::process::Command::new("git")
        .args(["commit", "-m", "admit handoff fixture inputs"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed.status.success(), "{committed:?}");
}

/// Compatibility wrapper for task fixtures; the shared Workspace owns setup and cleanup.
pub(crate) struct ScopedWorkspace(pub(crate) PathBuf, Workspace);

impl ScopedWorkspace {
    pub(crate) fn new(_prefix: &str) -> Self {
        let workspace = Workspace::new();
        init_git(workspace.root());
        Self(workspace.root().to_path_buf(), workspace)
    }

    pub(crate) fn write(&self, path: &str, content: &str) {
        self.1.write(path, content);
    }
}

pub(crate) fn init_git(root: &Path) {
    let initialized = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "could not initialize ACDD fixture repository: {}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let _ = std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "ACDD test"])
        .current_dir(root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "acdd-test@example.invalid"])
        .current_dir(root)
        .output();
    let initial = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "initial fixture baseline",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        initial.status.success(),
        "could not create ACDD fixture baseline: {}",
        String::from_utf8_lossy(&initial.stderr)
    );
}

pub(crate) fn git_head(root: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not read ACDD fixture HEAD: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn review_proof(decision: &str) -> serde_json::Value {
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"public-seam verification"}),
                )
            })
            .collect();
    json!({"review_proof":{"decision":decision,"contours":contours}})
}
pub(crate) fn passing_review_proof() -> serde_json::Value {
    review_proof("pass")
}
pub(crate) fn evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) -> Evidence {
    let stage = task.stage.as_str();
    Evidence {
        task_id: task.task_id.clone(),
        stage: task.stage.clone(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        // Snapshot and baseline hashes are engine-owned acceptance data, never test fixture input.
        commit: None,
        candidate_baseline_head: None,
        proof: match stage {
            "contract" => {
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
            }
            "build" => {
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}})
            }
            "review" => passing_review_proof(),
            "deliver" => json!({"delivery_proof":{}}),
            _ => json!({"none_proof":{}}),
        },
    }
}
pub(crate) fn pinned_candidate(root: &std::path::Path, task_id: &str) -> String {
    let reference = format!("refs/forge/snapshots/{}", task_id.replace(':', "/"));
    let output = std::process::Command::new("git")
        .args(["rev-parse", &reference])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
pub(crate) const SPEC: &str = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\nscope_roots: [src/, tests/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";
pub(crate) fn fixture() -> (ScopedWorkspace, TasksStore, Milestone) {
    let root = ScopedWorkspace::new("forge_tasks");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-test.md", SPEC);
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    (root, store, milestone)
}

// Shared indexed-workspace fixtures for task boundary and reachability tests.

pub(crate) fn prepare_indexed_boundary_workspace(
    api: &str,
    caller: &str,
    seam: &str,
    expected_target: &str,
    test_helper_source: Option<&str>,
    expected_intermediate: Option<&str>,
) -> ScopedWorkspace {
    let root = ScopedWorkspace::new("forge_current_boundary");
    root.write(
        "Cargo.toml",
        "[package]\nname = \"boundary_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write(
        "src/lib.rs",
        if expected_intermediate.is_some() {
            "pub mod api;\npub mod caller;\npub mod other;\npub mod test_helpers;\n#[cfg(test)]\nmod seam;\n"
        } else {
            "pub mod api;\npub mod caller;\npub mod other;\n#[cfg(test)]\nmod seam;\n"
        },
    );
    root.write("src/api.rs", api);
    root.write("src/caller.rs", caller);
    root.write("src/other.rs", "pub fn route() -> usize { 2 }\n");
    root.write("src/seam.rs", seam);
    if let Some(test_helper_source) = test_helper_source {
        root.write("src/test_helpers.rs", test_helper_source);
    }
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/milestones/010-boundary.md", "---\nid: m-current-boundary\ntitle: Current boundary evidence\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve current seam and public signature boundaries\nproof_policy: seam-test-first\nscope: [src/api.rs, src/seam.rs]\nscope_roots: [src/, tests/]\n```\n");
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    let has_expected_witness = if let Some(intermediate) = expected_intermediate {
        let exact_witness: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM edges test_edge JOIN nodes test ON test.node_hash=test_edge.src_hash JOIN path_dictionary test_path ON test_path.path_id=test.path_id JOIN nodes helper ON helper.node_hash=test_edge.dst_hash JOIN path_dictionary helper_path ON helper_path.path_id=helper.path_id JOIN edges helper_edge ON helper_edge.src_hash=helper.node_hash JOIN nodes target ON target.node_hash=helper_edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE test_edge.kind='calls' AND helper_edge.kind='calls' AND test.name='test_connected' AND test.kind='function' AND test_path.path='src/seam.rs' AND test.is_test=1 AND helper.name=?2 AND helper.kind='function' AND helper_path.path='src/test_helpers.rs' AND helper.is_test=1 AND target.name=?1 AND target.kind='function' AND target_path.path='src/api.rs' AND target.is_test=0",
                [expected_target, intermediate],
                |row| row.get(0),
            )
            .unwrap();
        exact_witness > 0
    } else {
        let seam_depth: Option<i64> = connection
            .query_row(
                "WITH RECURSIVE reachable(node_hash,depth) AS (SELECT node_hash,0 FROM nodes WHERE name='test_connected' AND is_test=1 UNION SELECT edge.dst_hash,reachable.depth+1 FROM reachable JOIN edges edge ON edge.src_hash=reachable.node_hash WHERE reachable.depth<4 AND edge.kind IN ('calls','imports')) SELECT MIN(reachable.depth) FROM reachable JOIN nodes target ON target.node_hash=reachable.node_hash JOIN path_dictionary path ON path.path_id=target.path_id WHERE path.path='src/api.rs' AND target.is_test=0 AND target.kind='function' AND target.name=?1",
                [expected_target],
                |row| row.get(0),
            )
            .unwrap();
        seam_depth.is_some_and(|depth| (1..=4).contains(&depth))
    };
    assert!(
        has_expected_witness,
        "fixture must persist the exact expected test-to-production call path"
    );
    drop(connection);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-boundary.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    root
}

pub(crate) fn indexed_boundary_workspace(
    api: &str,
    caller: &str,
    seam: &str,
    expected_target: &str,
) -> ScopedWorkspace {
    let root = prepare_indexed_boundary_workspace(api, caller, seam, expected_target, None, None);
    tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
    root
}

pub(crate) fn indexed_test_path_helper_workspace(
    api: &str,
    test_helper_source: &str,
) -> ScopedWorkspace {
    let root = prepare_indexed_boundary_workspace(
        api,
        "",
        "use crate::test_helpers::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n",
        "route",
        Some(test_helper_source),
        Some("helper"),
    );
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    root
}

pub(crate) fn submit_indexed_boundary(
    root: &ScopedWorkspace,
    stage: &str,
    seam: &str,
) -> anyhow::Result<Value> {
    let task_id = "forge-mcp/m-current-boundary:routing";
    let task = tasks::store_for_task(&root.0, task_id)?.inspect(task_id)?;
    let mut proof = evidence(&task);
    proof.proof = if stage == "contract" {
        json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}})
    } else {
        json!({"command_proof":{"command":"cargo test --test connected","exit_code":0,"tests_passed":1,"tests_failed":0}})
    };
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: stage.into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof)?,
            findings: None,
        },
    )
}

pub(crate) fn commit_fixture_baseline(root: &ScopedWorkspace) {
    let added = std::process::Command::new("git")
        .args(["add", "--all"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(added.status.success());
    let committed = std::process::Command::new("git")
        .args(["commit", "--quiet", "-m", "fixture source baseline"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "could not commit fixture baseline: {}",
        String::from_utf8_lossy(&committed.stderr)
    );
}

pub(crate) fn add_boundary_edge_fixtures(root: &ScopedWorkspace) {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES('acdd boundary fixture')",
            [],
        )
        .unwrap();
    let evidence: i64 = connection
        .query_row(
            "SELECT evidence_id FROM coverage_evidence WHERE evidence='acdd boundary fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for (path, name, relation) in [
        ("src/possible.rs", "possible_route", "possible_calls"),
        ("src/implements.rs", "implements_route", "implements"),
        ("src/overrides.rs", "overrides_route", "overrides"),
    ] {
        let inserted = connection
            .execute(
                "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) \
                 SELECT caller.node_hash,?1,target.node_hash,caller.path_id,caller.line,?2,?2,1 \
                 FROM nodes caller JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
                 JOIN nodes target ON target.name='route_task' AND target.line=1 \
                 JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
                 WHERE caller_path.path=?3 AND caller.name=?4 AND target_path.path='src/routing.rs'",
                rusqlite::params![relation, evidence, path, name],
            )
            .unwrap();
        assert_eq!(
            inserted, 1,
            "fixture relation {relation} must target the public route_task symbol"
        );
    }
}

pub(crate) fn method_caller_edge_count(root: &ScopedWorkspace) -> i64 {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .query_row(
            "SELECT COUNT(*) FROM edges edge \
             JOIN nodes target ON target.node_hash=edge.dst_hash AND target.kind='method' AND target.name='route_task' \
             JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
             JOIN nodes caller ON caller.node_hash=edge.src_hash AND caller.name='invoke_route' AND caller.is_test=0 \
             JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
             WHERE target_path.path='src/routing.rs' AND caller_path.path='src/caller.rs' \
               AND edge.kind IN ('calls','imports','possible_calls','implements','overrides')",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

pub(crate) fn function_caller_edge_count(root: &ScopedWorkspace, path: &str, name: &str) -> i64 {
    let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
    connection
        .query_row(
            "SELECT COUNT(*) FROM edges edge \
             JOIN nodes target ON target.node_hash=edge.dst_hash AND target.kind='function' AND target.name=?2 \
             JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
             JOIN nodes caller ON caller.node_hash=edge.src_hash AND caller.is_test=0 \
             JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
              WHERE target_path.path=?1 AND caller_path.path IN ('src/client.rs','src/client.ts','src/client.js','src/client.py') \
               AND edge.kind IN ('calls','imports','possible_calls','implements','overrides')",
            rusqlite::params![path, name],
            |row| row.get(0),
        )
        .unwrap()
}
