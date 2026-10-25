use super::*;

#[test]
fn signature_boundary_protects_implicitly_public_trait_methods() {
    let anchor = "pub fn anchor() -> usize { 1 }\n";
    let connected =
        "use crate::api::anchor;\n#[test]\nfn test_connected() { let actual = anchor(); assert_eq!(actual, 1); }\n";
    let caller = "use crate::api::Router;\npub fn invoke_route(router: &dyn Router) -> usize { Router::route(router) }\n";
    let mut failures = Vec::new();
    for (case, baseline, changed, reject) in [
        ("default_trait_method", "pub trait Router { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> u64 { 1 } }", true),
        ("declared_trait_method", "pub trait Router { fn route(&self) -> usize; }", "pub trait Router { fn route(&self) -> u64; }", true),
        ("trait_body_only", "pub trait Router { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize { 2 } }", false),
        ("private_inherent_method", "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize { 1 } }\npub struct Local; impl Local { fn private_route(&self, value: usize) -> usize { value } }", false),
        ("trait_impl_body_only", "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 1 } }", "pub trait Router { fn route(&self) -> usize; }\npub struct Local; impl Router for Local { fn route(&self) -> usize { 2 } }", false),
        ("crate_visible_trait", "pub(crate) trait Router { fn route(&self) -> usize { 1 } }", "pub(crate) trait Router { fn route(&self) -> u64 { 1 } }", true),
        ("qualified_visible_trait", "pub(in crate) trait Router { fn route(&self) -> usize { 1 } }", "pub(in crate) trait Router { fn route(&self) -> u64 { 1 } }", true),
    ] {
        for stage in ["contract", "build"] {
            let root = indexed_boundary_workspace(
                &format!("{anchor}{baseline}\n"),
                caller,
                connected,
                "anchor",
            );
            let connection = rusqlite::Connection::open(root.0.join(".forge/code-map.sqlite")).unwrap();
            let exact_calls: i64 = connection.query_row(
                "SELECT COUNT(*) FROM edges edge JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary tp ON tp.path_id=target.path_id JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN path_dictionary cp ON cp.path_id=caller.path_id WHERE edge.kind='calls' AND target.name='route' AND target.kind='method' AND tp.path='src/api.rs' AND caller.name='invoke_route' AND cp.path='src/caller.rs' AND caller.is_test=0",
                [], |row| row.get(0)).unwrap();
            assert!(exact_calls > 0, "{case}/{stage}: fixture must persist the exact external UFCS call");
            drop(connection);
            if stage == "build" {
                submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected").unwrap();
                tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
            }
            root.write("src/api.rs", &format!("{anchor}{changed}\n"));
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
            let result = submit_indexed_boundary(&root, stage, "src/seam.rs::test_connected");
            match (reject, result) {
                (true, Err(error)) => {
                    assert!(error.to_string().contains("TASK_SCOPE_INCOMPLETE"), "{case}/{stage}: {error}");
                    assert!(error.to_string().contains("src/caller.rs"), "{case}/{stage}: {error}");
                    assert_eq!(tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing").unwrap().inspect("forge-mcp/m-current-boundary:routing").unwrap().stage, stage);
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
fn signature_boundary_uses_exact_method_baseline_across_body_edits_and_reindex() {
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

    for (case, expected_error, refresh_index) in [
        ("same_named_methods", Some("TASK_SCOPE_INCOMPLETE"), false),
        ("body_only", None, false),
        ("refreshed_index", Some("TASK_SCOPE_INCOMPLETE"), true),
    ] {
        let root = ScopedWorkspace::new("forge_tiered_signature_identity");
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
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        let original_caller_edges = method_caller_edge_count(&root);
        assert!(
            original_caller_edges > 0,
            "fixture must index the external call to Router::route_task"
        );
        tasks::manage(
            &root.0,
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let task_id = "forge-mcp/m-tiered-signature:route-task";
        tasks::claim(
            &root.0,
            serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut contract_evidence = evidence(&task);
        contract_evidence.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "contract".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(contract_evidence).unwrap(),
                findings: None,
            },
        )
        .unwrap();
        let claim = tasks::claim(
            &root.0,
            serde_json::from_value(
                json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(claim["stage"], "build");

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
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
            let refreshed_caller_edges = method_caller_edge_count(&root);
            assert!(
                refreshed_caller_edges >= original_caller_edges,
                "reindex must preserve the external Router::route_task caller edge: before={original_caller_edges}, after={refreshed_caller_edges}"
            );
        }

        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        let mut build_evidence = evidence(&task);
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
        let command = profile.commands.get("test").unwrap().command();
        build_evidence.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "build".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(build_evidence).unwrap(),
                findings: None,
            },
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
    let root = ScopedWorkspace::new("forge_tiered_renamed_boundary");
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
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let baseline_edges = function_caller_edge_count(&root, "src/api.rs", "fetch");
    assert!(
        baseline_edges > 0,
        "baseline index must include src/client.rs -> fetch"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-rename.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-rename:rename-fetch";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/api.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    let task_store = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let accepted_contract: String = task_store
        .query_row(
            "SELECT result FROM task_submissions WHERE task_id=?1 ORDER BY revision DESC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let accepted_contract: serde_json::Value = serde_json::from_str(&accepted_contract).unwrap();
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
    let baseline = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(baseline.status.success());
    assert_eq!(
        accepted_contract["boundary_callers"]["baseline_commit"],
        String::from_utf8_lossy(&baseline.stdout).trim(),
        "caller evidence must be pinned to the Git baseline accepted by the contract gate"
    );
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.rs",
        "pub fn fetch_by_id(id: usize) -> usize { id }\n#[cfg(test)]\nmod tests { use crate::api::fetch_by_id; #[test] fn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); } }\n",
    );
    root.write("tests/connected.rs", "use tiered_rename_fixture::api::fetch_by_id;\n#[test]\nfn test_connected() { let actual = fetch_by_id(1); assert_eq!(actual, 1); }\n");
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let old_edges_after_reindex = function_caller_edge_count(&root, "src/api.rs", "fetch");
    assert_eq!(
        old_edges_after_reindex, 0,
        "reindex must remove every edge to the renamed fetch symbol before boundary validation"
    );

    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
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
fn signature_boundary_covers_exported_typescript_callables() {
    let root = ScopedWorkspace::new("forge_tiered_typescript_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write(
        "src/api.ts",
        "export function submit(id: number): number { return id; }\n",
    );
    root.write(
        "src/client.ts",
        "import { submit } from './api';\nexport function load(): number { return submit(1); }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_typescript_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_typescript_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
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
        "docs/milestones/010-tiered-typescript.md",
        "---\nid: m-tiered-typescript\ntitle: TypeScript API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-submit\ntarget: Protect exported TypeScript callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.ts, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.ts", "submit") > 0,
        "baseline index must resolve the TypeScript external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-typescript.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-typescript:protect-submit";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.ts",
        "export function submit(id: string): number { return id.length; }\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.ts", "submit") > 0,
        "reindex must retain the caller edge to the same exported symbol"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("exported TypeScript API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.ts"),
        "external TypeScript caller must explain the rejection: {error}"
    );
}

#[test]
fn signature_boundary_covers_public_python_callables() {
    let root = ScopedWorkspace::new("forge_tiered_python_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write(
        "src/api.py",
        "def fetch(id: int) -> int:\n    return id\n\ndef test_connected():\n    actual = fetch(1)\n    assert actual == 1\n",
    );
    root.write(
        "src/client.py",
        "from api import fetch\n\ndef load():\n    return fetch(0)\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_python_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_python_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
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
        "docs/milestones/010-tiered-python.md",
        "---\nid: m-tiered-python\ntitle: Python API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-fetch\ntarget: Protect public Python callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.py, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.py", "fetch") > 0,
        "baseline index must resolve the Python external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-python.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-python:protect-fetch";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.py",
        "def fetch(id: str) -> int:\n    return len(id)\n\ndef test_connected():\n    actual = fetch(\"1\")\n    assert actual == 1\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.py", "fetch") > 0,
        "reindex must retain the caller edge to the same public Python callable"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("public Python API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.py"),
        "external Python caller must explain the rejection: {error}"
    );
}

#[test]
fn signature_boundary_covers_exported_javascript_callables() {
    let root = ScopedWorkspace::new("forge_tiered_javascript_boundary");
    root.write("src/lib.rs", "pub mod routing;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests { use crate::routing::route_task; #[test] fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); } }\n");
    root.write("src/api.js", "export function submit(id) { return id; }\n");
    root.write(
        "src/client.js",
        "import { submit } from './api.js';\nexport function load() { return submit(1); }\n",
    );
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_javascript_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("tests/connected.rs", "use tiered_javascript_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
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
        "docs/milestones/010-tiered-javascript.md",
        "---\nid: m-tiered-javascript\ntitle: JavaScript API boundary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: protect-submit\ntarget: Protect exported JavaScript callable signatures\nproof_policy: seam-test-first\nscope: [src/routing.rs, src/api.js, tests/connected.rs, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.js", "submit") > 0,
        "baseline index must resolve the JavaScript external caller"
    );
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-javascript.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-javascript:protect-submit";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    )
    .unwrap();
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut contract = evidence(&task);
    contract.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(contract).unwrap(),
            findings: None,
        },
    )
    .unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();

    root.write(
        "src/api.js",
        "export function submit(id, label) { return id; }\n",
    );
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    assert!(
        function_caller_edge_count(&root, "src/api.js", "submit") > 0,
        "reindex must retain the caller edge to the same exported JavaScript callable"
    );
    let task = tasks::store_for_task(&root.0, task_id)
        .unwrap()
        .inspect(task_id)
        .unwrap();
    let mut build = evidence(&task);
    let profile =
        contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
    let command = profile.commands.get("test").unwrap().command();
    build.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
    let result = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(build).unwrap(),
            findings: None,
        },
    );
    let error =
        result.expect_err("exported JavaScript API changes must consult indexed external callers");
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.to_string().contains("src/client.js"),
        "external JavaScript caller must explain the rejection: {error}"
    );
}

#[test]
fn boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths() {
    let root = ScopedWorkspace::new("forge_tiered_alias_proof_kinds");
    root.write(
        "Cargo.toml",
        "[package]\nname = \"tiered_alias_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write("src/lib.rs", "pub mod routing;\npub mod caller;\n");
    root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n}\n");
    root.write(
        "src/caller.rs",
        "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task() }\n",
    );
    root.write("tests/connected.rs", "use tiered_alias_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
    root.write(
        "AGENTS.md",
        "# Instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write("forge-mcp.yaml", "roots: [src, tests]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n");
    root.write(
        "custom_profile.yaml",
        "gates:\n  - id: spec\n    role: contract_author\n    proof: contract\n    sha_snapshot: true\n  - id: implement\n    role: builder\n    proof: command\n    sha_snapshot: true\n  - id: inspect\n    role: independent_reviewer\n    proof: review\n    reject_to: implement\n    independent_from: implement\n    contours: standard\n  - id: archive\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [inspect]\n    reject_to: implement\ncommands:\n  rust-check-and-test:\n    command: pytest\n    description: Preserve ordinary scoped paths for non-Cargo profiles.\n  python-quality: ruff\n",
    );
    let milestone = "---\nid: m-tiered-alias\ntitle: Proof-kind aliases\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve routing API boundaries\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs]\nscope_roots: [src/, tests/]\n```\n";
    root.write("docs/milestones/010-tiered-alias.md", milestone);
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered-alias.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-tiered-alias:routing";
    tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"spec","worker_id":"spec-author","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();
    let store = tasks::store_for_task(&root.0, task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let mut proof = evidence(&task);
    proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "spec".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let claim = tasks::claim(
        &root.0,
        serde_json::from_value(
            json!({"task_id":task_id,"stage":"implement","worker_id":"builder","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();
    let scoped = &claim["workflow_guidance"]["scoped_commands"];
    assert!(
        scoped.as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["name"] == "rust-check-and-test"
                    && entry["test_path"] == "src/routing.rs"
                    && entry["command"] == "pytest src/routing.rs"
            }) && entries.iter().any(|entry| {
                entry["name"] == "python-quality"
                    && entry["test_path"] == "src/routing.rs"
                    && entry["command"] == "ruff src/routing.rs"
            })
        }),
        "scoped guidance must include every matching configured command with its ordinary test path: {scoped}"
    );

    root.write(
        "src/routing.rs",
        "pub fn route_task(value: usize) -> usize { value + 1 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n}\n",
    );
    root.write("tests/connected.rs", "use tiered_alias_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
    let task = store.inspect(task_id).unwrap();
    let mut proof = evidence(&task);
    proof.proof = json!({"command_proof":{"command":"pytest src/routing.rs","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "implement".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("TASK_SCOPE_INCOMPLETE"),
        "{error}"
    );
    assert_eq!(store.inspect(task_id).unwrap().stage, "implement");
}

#[test]
fn commit_scope_validation_checks_exact_milestone_and_non_empty_scope() {
    use contextunity_forge_mcp::core::tasks::commit_files_within_scope;

    let milestone_ref = "docs/milestones/010-repack.md";
    let scope = vec!["src/lib.rs".to_string()];

    // Exact milestone file matches
    assert!(commit_files_within_scope(
        &["docs/milestones/010-repack.md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));

    // Suffixes or file basenames in root must NOT match milestone_ref
    assert!(!commit_files_within_scope(
        &["010-repack.md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));
    assert!(!commit_files_within_scope(
        &[
            "notdocs/milestones/010-repack.md".into(),
            "src/lib.rs".into()
        ],
        &scope,
        Some(milestone_ref)
    ));
    assert!(!commit_files_within_scope(
        &["md".into(), "src/lib.rs".into()],
        &scope,
        Some(milestone_ref)
    ));

    // Normalized variants of the exact same path do match
    assert!(commit_files_within_scope(
        &["./docs/milestones/010-repack.md".into()],
        &[],
        Some(milestone_ref)
    ));
    assert!(commit_files_within_scope(
        &["docs\\milestones\\010-repack.md".into()],
        &[],
        Some(milestone_ref)
    ));

    // Protected paths under .forge/acdd/** and .forge/frameworks/** are NOT treated as runtime files:
    // they require explicit scope admission.
    assert!(!commit_files_within_scope(
        &[".forge/acdd/profile.yaml".into()],
        &scope,
        Some(milestone_ref),
    ));
    assert!(!commit_files_within_scope(
        &[".forge/frameworks/rules.yaml".into()],
        &scope,
        Some(milestone_ref),
    ));
    // If explicitly admitted in task scope, they pass:
    assert!(commit_files_within_scope(
        &[".forge/acdd/profile.yaml".into()],
        &[".forge/acdd/profile.yaml".into()],
        Some(milestone_ref),
    ));

    // Internal runtime artifacts (sqlite, lock, log, tasks runtime, checkpoints) pass without scope:
    assert!(commit_files_within_scope(
        &[".forge/tasks.sqlite".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/tasks.lock".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/checkpoints.json".into()],
        &[],
        Some(milestone_ref),
    ));
    assert!(commit_files_within_scope(
        &[".forge/tasks/state.sqlite".into()],
        &[],
        Some(milestone_ref),
    ));
}

#[test]
fn validate_commit_for_task_enforces_scope_and_ancestry_invariants() {
    use contextunity_forge_mcp::core::tasks::validate_commit_for_task;

    let root = ScopedWorkspace::new("forge_validate_commit_invariants");
    root.write("src/lib.rs", "pub fn base() {}\n");

    // Commit 1: base commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "src/lib.rs",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Baseline commit",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let base_sha = git_head(&root.0);

    // Commit 2: intermediate commit C1
    root.write("src/lib.rs", "pub fn base() {}\npub fn c1() {}\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Intermediate commit C1",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let c1_sha = git_head(&root.0);

    // Commit 3: delivery commit D
    root.write(
        "src/lib.rs",
        "pub fn base() {}\npub fn c1() {}\npub fn delivery() {}\n",
    );
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Delivery commit D",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let d_sha = git_head(&root.0);

    let scope = vec!["src/lib.rs".to_string()];
    let milestone_ref = "docs/milestones/010-test.md";

    // 1. Delivery commit D is valid when accepted_delivery is D
    assert!(validate_commit_for_task(
        &root.0,
        &d_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha)
    )
    .is_ok());

    // 2. Intermediate commit C1 is REJECTED when accepted_delivery is D (C1 is an ancestor of D)
    let c1_err = validate_commit_for_task(
        &root.0,
        &c1_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        c1_err.to_string().contains("TASK_COMMIT_OBSOLETE"),
        "expected obsolete commit error, got: {c1_err}"
    );

    // 3. Baseline commit is REJECTED (does not advance beyond baseline)
    let base_err = validate_commit_for_task(
        &root.0,
        &base_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        base_err.to_string().contains("TASK_COMMIT_OBSOLETE"),
        "expected obsolete baseline error, got: {base_err}"
    );

    // 4. Empty scope rejects commits that touch files outside scope (such as src/lib.rs):
    let outside_err = validate_commit_for_task(
        &root.0,
        &d_sha,
        &[],
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        outside_err
            .to_string()
            .contains("TASK_COMMIT_OUTSIDE_SCOPE"),
        "expected outside scope error for empty scope on src/lib.rs, got: {outside_err}"
    );

    // 5. Commits that only modify the milestone doc (or runtime files) fail with TASK_COMMIT_IGNORES_SCOPE
    root.write("docs/milestones/010-test.md", "# Milestone\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "docs/milestones/010-test.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Milestone doc only",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let doc_only_sha = git_head(&root.0);
    let empty_scope_err = validate_commit_for_task(
        &root.0,
        &doc_only_sha,
        &[],
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        empty_scope_err
            .to_string()
            .contains("TASK_COMMIT_IGNORES_SCOPE"),
        "expected ignores scope error for empty scope, got: {empty_scope_err}"
    );

    // 6. Divergent commit with different changes in scope MUST fail with TASK_COMMIT_TREE_MISMATCH
    root.write(
        "src/lib.rs",
        "pub fn base() {}\npub fn malicious_divergent() {}\n",
    );
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Divergent commit with different changes in scope",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let d_alt_sha = git_head(&root.0);
    let mismatch_err = validate_commit_for_task(
        &root.0,
        &d_alt_sha,
        &scope,
        Some(milestone_ref),
        Some(&d_sha),
        Some(&base_sha),
    )
    .unwrap_err();
    assert!(
        mismatch_err
            .to_string()
            .contains("TASK_COMMIT_TREE_MISMATCH"),
        "expected tree mismatch error for divergent commit, got: {mismatch_err}"
    );

    // 7. git_scoped_tree_changed compares full tree when scope is empty
    assert!(
        contextunity_forge_mcp::core::git::git_scoped_tree_changed(
            &root.0,
            &d_sha,
            &d_alt_sha,
            &[]
        )
        .unwrap(),
        "git_scoped_tree_changed with empty scope must detect full tree difference"
    );
    assert!(
        !contextunity_forge_mcp::core::git::git_scoped_tree_changed(&root.0, &d_sha, &d_sha, &[])
            .unwrap(),
        "git_scoped_tree_changed with empty scope must return false for identical commits"
    );
}
