use super::*;

fn check_implicitly_public_trait_method_cases(cases: &[(&str, &str, &str, bool)]) {
    assert_safe_git_environment();
    let anchor = "pub fn anchor() -> usize { 1 }\n";
    let connected =
        "use crate::api::anchor;\n#[test]\nfn test_connected() { let actual = anchor(); assert_eq!(actual, 1); }\n";
    let caller = "use crate::api::Router;\npub fn invoke_route(router: &dyn Router) -> usize { Router::route(router) }\n";
    let mut failures = Vec::new();
    let mut fixtures = HashMap::new();
    for &(case, baseline, changed, reject) in cases {
        let fixture = fixtures.entry(baseline).or_insert_with(|| {
            let root = indexed_boundary_workspace(
                &format!("{anchor}{baseline}\n"),
                caller,
                connected,
                "anchor",
                "src/seam.rs::test_connected",
            );
            let stages: Vec<String> = {
                let harness = TaskHarness::attach(&root, "forge-mcp/m-current-boundary:routing");
                harness
                    .executable_gates()
                    .unwrap()
                    .into_iter()
                    .filter(|gate| {
                        matches!(
                        gate.proof.kind(),
                        Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Contract)
                            | Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Command)
                    )
                    })
                    .map(|gate| gate.id)
                    .collect()
            };
            let mut snapshots = Vec::new();
            for (index, stage) in stages.iter().enumerate() {
                snapshots.push((stage.clone(), FixtureSnapshot::capture(&root)));
                if index + 1 == stages.len() {
                    break;
                }
                submit_indexed_boundary(&root, stage, "src/seam.rs::test_connected").unwrap();
                TaskHarness::claim_at(
                    root.root(),
                    "forge-mcp/m-current-boundary:routing",
                    &stages[index + 1],
                    "builder",
                )
                .unwrap();
            }
            (root, snapshots)
        });
        for (stage, snapshot) in &fixture.1 {
            snapshot.restore(&fixture.0);
            let exact_calls = TaskHarness::external_method_caller_edges(
                fixture.0.root(),
                ("src/api.rs", "route"),
                ("src/caller.rs", "invoke_route"),
            )
            .unwrap();
            assert!(
                exact_calls > 0,
                "{case}/{stage}: fixture must persist the exact external UFCS call"
            );
            fixture
                .0
                .write("src/api.rs", &format!("{anchor}{changed}\n"));
            let result = submit_indexed_boundary(&fixture.0, stage, "src/seam.rs::test_connected");
            match (reject, result) {
                (true, Err(error)) => {
                    assert!(error.to_string().contains("TASK_SCOPE_INCOMPLETE"), "{case}/{stage}: {error}");
                    assert!(error.to_string().contains("src/caller.rs"), "{case}/{stage}: {error}");
                    assert_eq!(tasks::store_for_task(fixture.0.root(), "forge-mcp/m-current-boundary:routing").unwrap().inspect("forge-mcp/m-current-boundary:routing").unwrap().stage, *stage);
                }
                (true, Ok(_)) => failures.push(format!("{case}/{stage}: changed implicitly public signature accepted despite exact external UFCS caller")),
                (false, Err(error)) => failures.push(format!("{case}/{stage}: unchanged public signature rejected: {error}")),
                (false, Ok(_)) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn signature_boundary_protects_implicitly_public_trait_methods() {
    check_implicitly_public_trait_method_cases(&[
        (
            "default_trait_method",
            "pub trait Router { fn route(&self) -> usize { 1 } }",
            "pub trait Router { fn route(&self) -> u64 { 1 } }",
            true,
        ),
        (
            "trait_body_only",
            "pub trait Router { fn route(&self) -> usize { 1 } }",
            "pub trait Router { fn route(&self) -> usize { 2 } }",
            false,
        ),
        (
            "declared_trait_method",
            "pub trait Router { fn route(&self) -> usize; }",
            "pub trait Router { fn route(&self) -> u64; }",
            true,
        ),
        (
            "private_inherent_method",
            "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self) -> usize { 1 } }",
            "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self, value: usize) -> usize { value } }",
            false,
        ),
    ]);
}

#[test]
fn signature_boundary_protects_trait_impl_body_and_restricted_visibility_traits() {
    check_implicitly_public_trait_method_cases(&[
        (
            "trait_impl_body_only",
            "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 1 } }",
            "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 2 } }",
            false,
        ),
        (
            "crate_visible_trait",
            "pub(crate) trait Router { fn route(&self) -> usize { 1 } }",
            "pub(crate) trait Router { fn route(&self) -> u64 { 1 } }",
            true,
        ),
        (
            "qualified_visible_trait",
            "pub(in crate) trait Router { fn route(&self) -> usize { 1 } }",
            "pub(in crate) trait Router { fn route(&self) -> u64 { 1 } }",
            true,
        ),
    ]);
}
#[test]
fn signature_boundary_uses_exact_method_baseline_across_body_edits_and_reindex() {
    assert_safe_git_environment();
    let routing_baseline = r#"pub fn route_task() -> usize { 1 }
pub struct Router;
impl Router {
    pub fn route_task(&self) -> usize { 10 }
}
pub struct Archive;
impl Archive {
    pub fn route_task(&self) -> usize { 20 }
}
pub fn test_only_route() -> usize { 3 }
#[cfg(test)]
mod tests {
    use crate::routing::route_task;

    #[test]
    fn test_connected() {
        let connected_target = route_task();
        let router = crate::routing::Router;
        let actual = router.route_task();
        assert_eq!(connected_target, 1);
        assert_eq!(actual, 10);
    }
}
"#;
    let caller_baseline = "use crate::routing::Router;\nimpl Router {\n    pub fn invoke_route(&self) -> usize { self.route_task() }\n}\n";
    let connected_test_baseline = "#[test]\nfn test_connected() { let router = tiered_fixture::routing::Router; let actual = router.route_task(); assert_eq!(actual, 10); }\n";

    let root = acdd_workspace("src/routing.rs::test_connected", "");
    root.write("src/lib.rs", "pub mod routing;\npub mod caller;\n");
    root.write("src/routing.rs", routing_baseline);
    root.write("src/caller.rs", caller_baseline);
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", connected_test_baseline);
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
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
    root.write(
            "docs/milestones/010-tiered.md",
            "---\nid: m-tiered-signature\ntitle: Tiered signature boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: route-task\ntarget: Protect the exact public method signature\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
        );
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    let original_caller_edges = boundary_caller_edge_count(
        &root,
        "method",
        "src/routing.rs",
        "route_task",
        &["src/caller.rs"],
        Some("invoke_route"),
    );
    assert!(
        original_caller_edges > 0,
        "fixture must index the external call to Router::route_task"
    );
    tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let task_id = "forge-mcp/m-tiered-signature:route-task";
    TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
    let task = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract_evidence = TaskHarness::default_evidence(&task);
    contract_evidence.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "contract", &contract_evidence),
    )
    .unwrap();
    let claim = TaskHarness::claim_at(root.root(), task_id, "build", "builder").unwrap();
    assert_eq!(claim["stage"], "build");

    let snapshot = FixtureSnapshot::capture(&root);
    for (case, expected_error, refresh_index) in [
        ("same_named_methods", Some("TASK_SCOPE_INCOMPLETE"), false),
        ("body_only", None, false),
        ("refreshed_index", Some("TASK_SCOPE_INCOMPLETE"), true),
    ] {
        snapshot.restore(&root);
        assert_eq!(
            boundary_caller_edge_count(
                &root,
                "method",
                "src/routing.rs",
                "route_task",
                &["src/caller.rs"],
                Some("invoke_route"),
            ),
            original_caller_edges
        );

        let mut routing = routing_baseline.to_owned();
        let mut caller = caller_baseline.to_owned();
        let mut connected_test = connected_test_baseline.to_owned();
        match case {
            "same_named_methods" | "refreshed_index" => {
                routing = routing.replace(
                    "impl Router {\n    pub fn route_task(&self) -> usize { 10 }",
                    "impl Router {\n    pub fn route_task(&self, value: usize) -> usize { 10 + value }",
                );
                assert_ne!(routing, routing_baseline, "Router method must change");
                caller = caller.replace("self.route_task()", "self.route_task(0)");
                connected_test =
                    connected_test.replace("router.route_task()", "router.route_task(0)");
            }
            "body_only" => {
                routing = routing.replace(
                    "impl Router {\n    pub fn route_task(&self) -> usize { 10 }",
                    "impl Router {\n    pub fn route_task(&self) -> usize { 11 }",
                );
                assert_ne!(routing, routing_baseline, "Router method body must change");
                connected_test =
                    connected_test.replace("assert_eq!(actual, 10)", "assert_eq!(actual, 11)");
            }
            _ => unreachable!("all signature scenarios are listed above"),
        }
        assert!(
            routing.contains("impl Archive {\n    pub fn route_task(&self) -> usize { 20 }"),
            "{case}: unchanged same-named Archive method must remain in the fixture"
        );
        if case == "body_only" {
            assert!(
                routing.contains("impl Router {\n    pub fn route_task(&self) -> usize { 11 }"),
                "body-only case must preserve Router's qualified signature"
            );
        } else {
            assert!(
                routing.contains(
                    "impl Router {\n    pub fn route_task(&self, value: usize) -> usize { 10 + value }"
                ),
                "{case}: only the externally called Router method signature changes"
            );
        }
        root.write("src/routing.rs", &routing);
        root.write("src/caller.rs", &caller);
        root.write("tests/connected.rs", &connected_test);

        if refresh_index {
            root.build_to(root.path(".forge/code-map.sqlite"));
            let refreshed_caller_edges = boundary_caller_edge_count(
                &root,
                "method",
                "src/routing.rs",
                "route_task",
                &["src/caller.rs"],
                Some("invoke_route"),
            );
            assert!(
                refreshed_caller_edges >= original_caller_edges,
                "reindex must preserve the external Router::route_task caller edge: before={original_caller_edges}, after={refreshed_caller_edges}"
            );
        }

        let task = tasks::store_for_task(root.root(), task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut build_evidence = TaskHarness::default_evidence(&task);
        build_evidence.proof = json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}});
        let result = tasks::submit(
            root.root(),
            TaskHarness::pass_request(task_id, "build", &build_evidence),
        );
        match (expected_error, result) {
            (Some(code), Err(error)) => {
                assert!(error.to_string().contains(code), "{case}: {error}");
                assert!(
                    error.to_string().contains("src/caller.rs"),
                    "{case}: external method caller must remain in the rejection: {error}"
                );
            }
            (Some(code), Ok(_)) => panic!("{case}: expected rejection with {code}"),
            (None, Err(error)) => panic!("{case}: body-only edit must pass: {error}"),
            (None, Ok(_)) => {}
        }
    }
}
#[test]
fn signature_boundary_keeps_contract_acceptance_callers_after_symbol_rename_and_reindex() {
    let root = acdd_workspace("src/api.rs::test_connected", "");
    root.write("src/lib.rs", "pub mod api;\npub mod client;\n");
    root.write(
        "src/api.rs",
        "pub fn fetch(id: usize) -> usize { id }\n#[cfg(test)]\nmod tests { use crate::api::fetch; #[test] fn test_connected() { let actual = fetch(1); assert_eq!(actual, 1); } }\n",
    );
    root.write(
        "src/client.rs",
        "use crate::api::fetch;\npub fn load() -> usize { fetch(0) }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_rename_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_rename_fixture::api::fetch;\n#[test]\nfn test_connected() { let actual = fetch(1); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
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
    root.write(
        "docs/milestones/010-tiered-rename.md",
        "---\nid: m-tiered-rename\ntitle: Renamed API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: rename-fetch\ntarget: Preserve every caller when renaming a public API\nproof_policy: seam-test-first\nscope: [src/api.rs, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    let baseline_edges = boundary_caller_edge_count(
        &root,
        "function",
        "src/api.rs",
        "fetch",
        &[
            "src/client.rs",
            "src/client.ts",
            "src/client.js",
            "src/client.py",
        ],
        None,
    );
    assert!(
        baseline_edges > 0,
        "baseline index must include src/client.rs -> fetch"
    );
    tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-rename.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-rename:rename-fetch";
    TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
    let task = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = TaskHarness::default_evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/api.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "contract", &contract),
    )
    .unwrap();
    let accepted_contract = TaskHarness::latest_submission_result(
        &tasks::store_for_task(root.root(), task_id).unwrap(),
        task_id,
    )
    .unwrap()
    .1;
    assert!(
        accepted_contract["boundary_callers"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["path"] == "src/api.rs"
                    && entry["name"] == "fetch"
                    && entry["caller_paths"]
                        .as_array()
                        .is_some_and(|paths| paths.iter().any(|path| path == "src/client.rs"))
            }),
        "contract acceptance must persist the stable-baseline external caller path"
    );
    let baseline = TaskHarness::revision(root.root(), "HEAD");
    assert!(baseline.status.success());
    assert_eq!(
        accepted_contract["boundary_callers"]["baseline_commit"],
        String::from_utf8_lossy(&baseline.stdout).trim(),
        "caller evidence must be pinned to the Git baseline accepted by the contract gate"
    );
    TaskHarness::claim_at(root.root(), task_id, "build", "builder").unwrap();

    root.write(
        "src/api.rs",
        "pub fn fetch_by_id(id: usize) -> usize { id }\n#[cfg(test)]\nmod tests { use crate::api::fetch_by_id; #[test] fn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); } }\n",
    );
    root.write("tests/connected.rs", "use tiered_rename_fixture::api::fetch_by_id;\n#[test]\nfn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); }\n");
    root.build_to(root.path(".forge/code-map.sqlite"));
    let old_edges_after_reindex = boundary_caller_edge_count(
        &root,
        "function",
        "src/api.rs",
        "fetch",
        &[
            "src/client.rs",
            "src/client.ts",
            "src/client.js",
            "src/client.py",
        ],
        None,
    );
    assert_eq!(
        old_edges_after_reindex, 0,
        "reindex must remove every edge to the renamed fetch symbol before boundary validation"
    );

    let task = tasks::store_for_task(root.root(), task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = TaskHarness::default_evidence(&task);
    build.proof = json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "build", &build),
    );
    let error = result.expect_err(
        "contract-accepted external callers must survive an API rename and index rebuild",
    );
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.rs"),
        "saved baseline caller path must explain the rejection: {error}"
    );
}
#[test]
fn signature_boundary_covers_public_callables_across_languages() {
    assert_safe_git_environment();
    let root = acdd_workspace("src/routing.rs::test_connected", "");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write(
        "src/api_typescript.ts",
        "export function submit(id: number): number { return id; }\n",
    );
    root.write(
        "src/client.ts",
        "import { submit } from './api_typescript';\nexport function load(): number { return submit(1); }\n",
    );
    root.write(
        "src/api_python.py",
        "def fetch(id: int) -> int:\n    return id\n\ndef test_connected():\n    actual = fetch(1)\n    assert actual == 1\n",
    );
    root.write(
        "src/client.py",
        "from api_python import fetch\n\ndef load():\n    return fetch(0)\n",
    );
    root.write(
        "src/api_javascript.js",
        "export function submit(id) { return id; }\n",
    );
    root.write(
        "src/client.js",
        "import { submit } from './api_javascript.js';\nexport function load() { return submit(1); }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_callables_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_callables_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write("tests/acdd.rs", "mod tasks;\n");
    root.write(
        "tests/acdd/tasks.rs",
        "#[test]\nfn task_suite_is_present() { assert_eq!(1, 1); }\n",
    );
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
    let milestone_ref = "docs/milestones/010-tiered-callables.md";
    root.write(
        milestone_ref,
        "---\nid: m-tiered-callables\ntitle: Public callable signature boundaries\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-callables\ntarget: Protect public callable signatures across supported languages\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api_typescript.ts, src/api_python.py, src/api_javascript.js, tests/connected.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    root.build_to(root.path(".forge/code-map.sqlite"));
    let callable_cases = [
        (
            "TypeScript",
            "src/api_typescript.ts",
            "submit",
            "src/client.ts",
            "export function submit(id: string): number { return id.length; }\n",
            "baseline index must resolve the TypeScript external caller",
            "reindex must retain the caller edge to the same exported TypeScript callable",
            "exported TypeScript API changes must consult indexed external callers",
        ),
        (
            "Python",
            "src/api_python.py",
            "fetch",
            "src/client.py",
            "def fetch(id: str) -> int:\n    return len(id)\n\ndef test_connected():\n    actual = fetch(\"1\")\n    assert actual == 1\n",
            "baseline index must resolve the Python external caller",
            "reindex must retain the caller edge to the same public Python callable",
            "public Python API changes must consult indexed external callers",
        ),
        (
            "JavaScript",
            "src/api_javascript.js",
            "submit",
            "src/client.js",
            "export function submit(id, label) { return id; }\n",
            "baseline index must resolve the JavaScript external caller",
            "reindex must retain the caller edge to the same exported JavaScript callable",
            "exported JavaScript API changes must consult indexed external callers",
        ),
    ];
    // Check baseline calls before any snapshot restore; each mutation row also
    // verifies the production index after reindexing below.
    for (language, api_path, callable_name, caller_path, _, baseline_expectation, _, _) in
        callable_cases
    {
        assert!(
            boundary_caller_edge_count(
                &root,
                "function",
                api_path,
                callable_name,
                &[caller_path],
                None,
            ) > 0,
            "{language}: {baseline_expectation}"
        );
    }
    tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(milestone_ref.to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    let task_id = "forge-mcp/m-tiered-callables:protect-callables";
    {
        let mut h = TaskHarness::attach(&root, task_id);
        h.advance_to("build").unwrap();
        h.claim_next().unwrap();
    }
    let snapshot = FixtureSnapshot::capture(&root);

    for (
        language,
        api_path,
        callable_name,
        caller_path,
        changed_api,
        _,
        reindex_expectation,
        rejection_expectation,
    ) in callable_cases
    {
        snapshot.restore(&root);
        root.write(api_path, changed_api);
        root.build_to(root.path(".forge/code-map.sqlite"));
        assert!(
            boundary_caller_edge_count(
                &root,
                "function",
                api_path,
                callable_name,
                &[caller_path],
                None,
            ) > 0,
            "{language}: {reindex_expectation}"
        );
        let task = tasks::store_for_task(root.root(), task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut build = TaskHarness::default_evidence(&task);
        build.proof = json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}});
        let error = tasks::submit(
            root.root(),
            TaskHarness::pass_request(task_id, "build", &build),
        )
        .expect_err(rejection_expectation);
        assert!(
            error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
            "{language}: {error}"
        );
        assert!(
            error.to_string().contains(caller_path),
            "{language}: external callable caller must explain the rejection: {error}"
        );
    }
}
