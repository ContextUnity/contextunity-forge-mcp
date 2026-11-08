use super::*;

// Shared indexed-workspace fixtures for task boundary and reachability tests.

pub(crate) fn prepare_indexed_boundary_workspace(
    api: &str,
    caller: &str,
    seam: &str,
    expected_target: &str,
    test_helper_source: Option<&str>,
    expected_intermediate: Option<&str>,
    seam_ref: &str,
) -> Workspace {
    let root = acdd_workspace(seam_ref, "");
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
    root.build();
    let connection = rusqlite::Connection::open(root.path(".forge/code-map.sqlite")).unwrap();
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
        root.root(),
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
    seam_ref: &str,
) -> Workspace {
    let root = prepare_indexed_boundary_workspace(
        api,
        caller,
        seam,
        expected_target,
        None,
        None,
        seam_ref,
    );
    crate::acdd::tasks::harness::TaskHarness::claim_at(
        root.root(),
        "forge-mcp/m-current-boundary:routing",
        "contract",
        "contract-author",
    )
    .unwrap();
    root
}

pub(crate) fn submit_indexed_boundary(
    root: &Workspace,
    stage: &str,
    seam: &str,
) -> anyhow::Result<Value> {
    let task_id = "forge-mcp/m-current-boundary:routing";
    let task = tasks::store_for_task(root.root(), task_id)?.inspect(task_id)?;
    let mut proof = evidence(&task);
    proof.proof = if stage == "contract" {
        json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}})
    } else {
        json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}})
    };
    tasks::submit(
        root.root(),
        crate::acdd::tasks::harness::TaskHarness::pass_request(task_id, stage, &proof),
    )
}

pub(crate) fn boundary_caller_edge_count(
    root: &Workspace,
    target_kind: &str,
    target_path: &str,
    target_name: &str,
    caller_paths: &[&str],
    caller_name: Option<&str>,
) -> i64 {
    let connection = rusqlite::Connection::open(root.path(".forge/code-map.sqlite")).unwrap();
    let caller_path_placeholders = (5..5 + caller_paths.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "SELECT COUNT(*) FROM edges edge \
         JOIN nodes target ON target.node_hash=edge.dst_hash \
         JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
         JOIN nodes caller ON caller.node_hash=edge.src_hash AND caller.is_test=0 \
         JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
         WHERE target.kind=?1 AND target.name=?2 AND target_path.path=?3 \
           AND (?4 = '' OR caller.name=?4) \
           AND caller_path.path IN ({caller_path_placeholders}) \
           AND edge.kind IN ('calls','imports','possible_calls','implements','overrides')"
    );
    let parameters = std::iter::once(target_kind)
        .chain([target_name, target_path, caller_name.unwrap_or("")])
        .chain(caller_paths.iter().copied());
    connection
        .query_row(&query, rusqlite::params_from_iter(parameters), |row| {
            row.get(0)
        })
        .unwrap()
}
