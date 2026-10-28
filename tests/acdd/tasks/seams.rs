use super::*;

#[test]
fn contract_seam_accepts_root_cargo_package_imports_from_integration_tests() {
    let root = prepare_indexed_boundary_workspace(
        "pub fn route() -> usize { 1 }\n",
        "",
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
        "route",
        None,
        None,
    );
    root.write(
        "tests/connected.rs",
        "use boundary_fixture::api::route;\n#[test]\nfn integration_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
    );
    commit_fixture_baseline(&root);
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
    ).unwrap();
    submit_indexed_boundary(&root, "contract", "tests/connected.rs").unwrap();
    assert_eq!(
        tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
            .unwrap()
            .inspect("forge-mcp/m-current-boundary:routing")
            .unwrap()
            .stage,
        "build"
    );
}

#[test]
fn contract_seam_requires_current_indexed_paths_at_every_hop() {
    let api = "pub fn route() -> usize { 1 }\n";
    let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\npub fn outer() -> usize { helper() }\n";
    let direct =
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n";
    let indirect =
        "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n";
    let mut failures = Vec::new();
    for (case, seam, refresh, reject) in [
        ("fresh_direct", "src/seam.rs::test_connected", false, false),
        (
            "fresh_two_hops",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
        ("three_hops", "src/seam.rs::test_connected", false, false),
        ("four_hops", "src/seam.rs::test_connected", false, false),
        (
            "removed_direct_call",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        ("removed_direct_call_file", "src/seam.rs", false, true),
        (
            "disconnected_helper",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        ("deleted_helper", "src/seam.rs::test_connected", false, true),
        ("deleted_target", "src/seam.rs::test_connected", false, true),
        ("unindexed_new_test", "src/seam.rs", false, true),
        (
            "refreshed_disconnected",
            "src/seam.rs::test_connected",
            true,
            true,
        ),
        (
            "refreshed_connected",
            "src/seam.rs::test_connected",
            true,
            false,
        ),
        (
            "unrelated_file_change",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
        (
            "retargeted_import",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "retargeted_local_alias",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "target_becomes_test",
            "src/seam.rs::test_connected",
            false,
            true,
        ),
        (
            "line_shift_only",
            "src/seam.rs::test_connected",
            false,
            false,
        ),
    ] {
        let local_alias = "use crate::api::route;\nuse crate::other::route as other_route;\n#[test]\nfn test_connected() { let handler = other_route; let handler = route; let actual = handler(); assert_eq!(actual, 1); }\n";
        let initial = if matches!(
            case,
            "fresh_two_hops" | "disconnected_helper" | "deleted_helper"
        ) {
            indirect.to_owned()
        } else if case == "retargeted_local_alias" {
            local_alias.to_owned()
        } else if matches!(case, "three_hops" | "four_hops") {
            let helper = if case == "four_hops" {
                "wrapper"
            } else {
                "outer"
            };
            direct
                .replace("api::route", &format!("caller::{helper}"))
                .replace("route()", &format!("{helper}()"))
        } else {
            direct.to_owned()
        };
        let caller_source = if case == "four_hops" {
            "use crate::api::route;\npub fn helper() -> usize { route() }\npub fn outer() -> usize { helper() }\npub fn wrapper() -> usize { outer() }\n"
        } else {
            caller
        };
        let root = indexed_boundary_workspace(api, caller_source, &initial, "route");
        match case {
            "removed_direct_call" | "removed_direct_call_file" | "refreshed_disconnected" => root
                .write(
                    "src/seam.rs",
                    "#[test]\nfn test_connected() { assert_eq!(1, 2); }\n",
                ),
            "disconnected_helper" => root.write(
                "src/caller.rs",
                "pub fn helper() -> usize { 2 }\npub fn outer() -> usize { helper() }\n",
            ),
            "deleted_helper" => root.write("src/caller.rs", "pub fn outer() -> usize { 2 }\n"),
            "deleted_target" => root.write("src/api.rs", "pub fn replacement() -> usize { 1 }\n"),
            "unindexed_new_test" => root.write(
                "src/seam.rs",
                &format!(
                    "#[test]\nfn test_connected() {{ assert_eq!(1, 2); }}\n{}",
                    direct.replace("test_connected", "test_new")
                ),
            ),
            "refreshed_connected" => root.write(
                "src/seam.rs",
                &direct
                    .replace("assert_eq!", "assert_ne!")
                    .replace(", 1)", ", 2)"),
            ),
            "unrelated_file_change" => {
                root.write("src/unrelated.rs", "pub fn unrelated() -> usize { 2 }\n")
            }
            "retargeted_import" => {
                root.write(
                    "src/seam.rs",
                    &direct.replace("use crate::api::route;", "use crate::other::route;"),
                );
                root.write("src/other.rs", "pub fn route() -> usize { 2 }\n");
            }
            "retargeted_local_alias" => root.write(
                "src/seam.rs",
                &local_alias.replace("let handler = route", "let handler = other_route"),
            ),
            "target_becomes_test" => {
                root.write("src/api.rs", "#[test]\npub fn route() -> usize { 1 }\n")
            }
            "line_shift_only" => root.write("src/seam.rs", &format!("// shifted line\n{direct}")),
            _ => {}
        }
        if refresh {
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        }
        let result = submit_indexed_boundary(&root, "contract", seam);
        match (reject, result) {
            (true, Err(error)) => {
                let error = error.to_string();
                let expected = if case == "refreshed_disconnected" {
                    "TASK_SEAM_DISCONNECTED"
                } else {
                    "TASK_SEAM_STALE"
                };
                if !error.contains(expected) {
                    failures.push(format!("{case}: expected {expected}, got: {error}"));
                }
                for diagnostic in [
                    "Calls discovered:",
                    "Coverage statuses:",
                    "Reached targets:",
                    "Stop reason:",
                ] {
                    if !error.contains(diagnostic) {
                        failures.push(format!(
                            "{case}: missing '{diagnostic}' diagnostic: {error}"
                        ));
                    }
                }
                if expected == "TASK_SEAM_STALE" && !error.contains("contextunity-forge-mcp scan") {
                    failures.push(format!(
                        "{case}: stale diagnostic omits refresh action: {error}"
                    ));
                }
                assert_eq!(
                    tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .inspect("forge-mcp/m-current-boundary:routing")
                        .unwrap()
                        .stage,
                    "contract"
                );
            }
            (true, Ok(_)) => failures.push(format!(
                "{case}: accepted a seam without a current indexed production path"
            )),
            (false, Err(error)) => failures.push(format!(
                "{case}: valid current indexed seam rejected: {error}"
            )),
            (false, Ok(_)) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn contract_seam_accepts_test_path_helpers_and_rejects_stale_hops() {
    let api = "pub fn route() -> usize { 1 }\n";
    let test_helper_source = "use crate::api::route;\npub fn helper() -> usize { route() }\n";
    let mut failures = Vec::new();

    let fresh = indexed_test_path_helper_workspace(api, test_helper_source);
    match submit_indexed_boundary(&fresh, "contract", "src/seam.rs::test_connected") {
        Ok(_) => {}
        Err(error) => failures.push(format!("fresh test-marked helper path rejected: {error}")),
    }

    let stale = indexed_test_path_helper_workspace(api, test_helper_source);
    stale.write(
        "src/test_helpers.rs",
        &test_helper_source.replace(
            "pub fn helper() -> usize { route() }",
            "fn helper() -> usize { 1 }",
        ),
    );
    match submit_indexed_boundary(&stale, "contract", "src/seam.rs::test_connected") {
        Err(error) if error.to_string().contains("TASK_SEAM_STALE") => {
            let error = error.to_string();
            assert!(error.contains("contextunity-forge-mcp scan"), "{error}");
            assert!(error.contains("Reached targets:"), "{error}");
        }
        Err(error) => failures.push(format!(
            "stale helper failed at the wrong boundary: {error}"
        )),
        Ok(_) => {
            failures.push("stale test-marked helper path was accepted without reindexing".into())
        }
    }
    let stale_stage = tasks::store_for_task(&stale.0, "forge-mcp/m-current-boundary:routing")
        .unwrap()
        .inspect("forge-mcp/m-current-boundary:routing")
        .unwrap()
        .stage;
    if stale_stage != "contract" {
        failures.push(format!(
            "stale helper rejection advanced task stage to {stale_stage}"
        ));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn contract_seam_validates_claimed_worktree_against_shared_primary_index() {
    let mut failures = Vec::new();
    for (case, disconnect_helper) in [("stale_test", false), ("stale_helper", true)] {
        let api = "pub fn route() -> usize { 1 }\n";
        let caller = "use crate::api::route;\npub fn helper() -> usize { route() }\n";
        let seam = if disconnect_helper {
            "use crate::caller::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n"
        } else {
            "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n"
        };
        let root = prepare_indexed_boundary_workspace(api, caller, seam, "route", None, None);
        let worktree = root.0.join("claimed-worktree");
        let added = std::process::Command::new("git")
            .args(["worktree", "add", "--quiet", "-b", "claimed-boundary"])
            .arg(&worktree)
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        tasks::claim(&root.0, serde_json::from_value(json!({"task_id":"forge-mcp/m-current-boundary:routing","stage":"contract","worker_id":"contract-author","worktree":worktree})).unwrap()).unwrap();
        if disconnect_helper {
            std::fs::write(
                worktree.join("src/caller.rs"),
                "pub fn helper() -> usize { 2 }\n",
            )
            .unwrap();
        } else {
            std::fs::write(
                worktree.join("src/seam.rs"),
                "#[test]\nfn test_connected() { assert_eq!(1, 2); }\n",
            )
            .unwrap();
        }
        writer::build(&worktree, &worktree.join(".forge/code-map.sqlite"), None).unwrap();
        let result = submit_indexed_boundary(&root, "contract", "src/seam.rs::test_connected");
        let Err(error) = result else {
            failures.push(format!("{case}: shared primary index attested a disconnected claimed worktree despite fresh local index"));
            continue;
        };
        assert!(
            error.to_string().contains("TASK_SEAM_DISCONNECTED"),
            "{case}: {error}"
        );
        assert_eq!(
            tasks::store_for_task(&root.0, "forge-mcp/m-current-boundary:routing")
                .unwrap()
                .inspect("forge-mcp/m-current-boundary:routing")
                .unwrap()
                .stage,
            "contract"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn tiered_task_context_enforces_indexed_seams_and_signature_boundaries() {
    let mut failures = Vec::new();
    for (stage, seam, change_signature, admit_caller, expected_error) in [
        (
            "contract",
            "tests/disconnected.rs",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_unrelated",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_missing",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "tests/deleted.rs::test_deleted",
            false,
            false,
            Some("TASK_SEAM_DISCONNECTED"),
        ),
        (
            "contract",
            "src/routing.rs::test_connected",
            true,
            false,
            Some("TASK_SCOPE_INCOMPLETE"),
        ),
        (
            "build",
            "src/routing.rs::test_connected",
            true,
            false,
            Some("TASK_SCOPE_INCOMPLETE"),
        ),
        ("build", "src/routing.rs::test_connected", true, true, None),
    ] {
        let root = ScopedWorkspace::new("forge_tiered_boundary");
        root.write("src/lib.rs", "pub mod routing;\npub mod caller;\npub mod possible;\npub mod implements;\npub mod overrides;\n");
        root.write("src/routing.rs", "pub fn route_task() -> usize { 1 }\npub mod archive { pub fn route_task() -> usize { 2 } }\npub fn test_only_route() -> usize { 3 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() {\n        let actual = route_task();\n        assert_eq!(actual, 1);\n    }\n    #[test]\n    fn test_unrelated() {\n        let actual = 3;\n        assert_eq!(actual, 3);\n    }\n}\n");
        root.write(
            "Cargo.toml",
            "[package]\nname = \"tiered_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        root.write(
            "src/caller.rs",
            "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task() }\n#[cfg(test)]\nmod tests {\n    use crate::caller::invoke_route;\n    #[test]\n    fn test_indirect() {\n        let actual = invoke_route();\n        assert_eq!(actual, 1);\n    }\n}\n",
        );
        root.write(
            "src/possible.rs",
            "pub fn possible_route() -> usize { 1 }\n",
        );
        root.write(
            "src/implements.rs",
            "pub fn implements_route() -> usize { 1 }\n",
        );
        root.write(
            "src/overrides.rs",
            "pub fn overrides_route() -> usize { 1 }\n",
        );
        root.write("tests/connected.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
        root.write("tests/acdd.rs", "mod tasks;\n");
        root.write("tests/acdd/tasks.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_nested_connected() { let actual = route_task(); assert_eq!(actual, 1); }\n");
        root.write(
            "tests/disconnected.rs",
            "#[test]\nfn test_disconnected() { assert_eq!(1, 1); }\n",
        );
        root.write("tests/external.rs", "use tiered_fixture::routing::test_only_route;\n#[test]\nfn external_test_only_caller() { let actual = test_only_route(); assert_eq!(actual, 3); }\n");
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
            ".forge/acdd/profile.yaml",
            include_str!("../../../.forge/acdd/profile.yaml"),
        );
        root.write("docs/adr/001-routing.md", "---\ntitle: Routing invariants\ndoc_type: adr\nstatus: accepted\n---\n# Routing invariants\n- INV-ROUTING: [`src.routing.route_task`](../../src/routing.rs#L1) remains observable through the public seam.\n- Decision: Admit every caller before changing the public signature.\n");
        let caller_scope = if admit_caller {
            ", src/caller.rs, src/possible.rs, src/implements.rs, src/overrides.rs"
        } else {
            ""
        };
        let specification = format!("---\nid: m-tiered\ntitle: Tiered boundary gates\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve the routing boundary\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs, tests/acdd/tasks.rs{caller_scope}]\nscope_roots: [src/, tests/]\n```\n");
        root.write("docs/milestones/010-tiered.md", &specification);
        commit_fixture_baseline(&root);
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        add_boundary_edge_fixtures(&root);
        tasks::manage(
            &root.0,
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let task_id = "forge-mcp/m-tiered:routing";
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
        let canonical_command =
            "cargo clippy --all-targets --all-features -- -D warnings && cargo test";
        if !profile
            .commands
            .values()
            .any(|command| command.command() == canonical_command)
        {
            failures.push(format!("{stage}/{seam}: default Rust profile must register static checks before cargo test"));
        }
        if !profile
            .commands
            .get("test")
            .is_some_and(|command| command.command() == "cargo test")
        {
            failures.push(format!("{stage}/{seam}: default Rust profile must retain the registered cargo test compatibility command"));
        }
        for (path, expected) in [
            ("src/main.rs", "--bin contextunity-forge-mcp"),
            ("src/caller.rs", "--lib"),
            ("src/routing.rs", "--lib"),
            ("tests/acdd/tasks.rs", "--test acdd"),
            ("tests/connected.rs", "--test connected"),
            ("tests/external.rs", "--test external"),
        ] {
            if profile.scope_args_for_path(path).as_deref() != Some(expected) {
                failures.push(format!(
                    "{stage}/{seam}: default profile must map {path} to valid Cargo scope args {expected:?}"
                ));
            }
        }
        if profile.roles.len() != 4 || !profile.roles.contains_key("architect") {
            failures.push(format!(
                "{stage}/{seam}: profile must include the three task roles and the milestone architect role"
            ));
        }
        for (gate, role, model) in [
            ("contract", "contract_author", "sol-6.1:high"),
            ("build", "builder", "gpt6-luna:xhigh"),
            ("review", "independent_reviewer", "sol-6.1:xhigh"),
            ("deliver", "independent_reviewer", "sol-6.1:xhigh"),
        ] {
            let role_definition = profile.role_for_stage(gate);
            if !role_definition.is_some_and(|(name, definition)| {
                name == role
                    && definition.effective_reuse_on_reject()
                    && definition.effective_mode()
                        == contextunity_forge_mcp::core::tasks::profile::RoleExecutionMode::Subagent
                    && definition
                        .models
                        .iter()
                        .any(|entry| entry.formatted() == model)
            }) {
                failures.push(format!("{stage}/{seam}: {gate} must use {role} with {model} and reusable worker authority"));
            }
        }
        let mut claim = tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap()).unwrap();
        let guidance = &claim["workflow_guidance"];
        if !guidance["rules"].as_array().unwrap().iter().any(|rule| {
            rule.as_str()
                .is_some_and(|rule| rule.starts_with("NO_PREFLIGHT_SUBAGENTS:"))
        }) {
            failures.push(format!(
                "{stage}/{seam}: task guidance must prohibit preflight subagents"
            ));
        }
        let has_build_handoff = guidance["role_guide"]
            .as_str()
            .is_some_and(|guide| guide.contains("build-targeted draft") || guide.contains("builder"))
            || guidance["steps"].as_array().is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.as_str()
                        .is_some_and(|step| step.contains("gate=\"build\"") || step.contains("gate=build"))
                })
            });
        if !has_build_handoff {
            failures.push(format!(
                "{stage}/{seam}: contract guidance must require the build-targeted blackboard handoff"
            ));
        }
        let template = &guidance["expected_evidence_template"];
        if !template.is_object() || !template.to_string().contains(task_id) {
            failures.push(format!(
                "{stage}/{seam}: claim must contain a task-bound expected evidence template"
            ));
        }
        let bundle = &claim["context_bundle"];
        if !bundle["adrs"].as_array().unwrap().iter().any(|adr| {
            adr["invariants_summary"].as_array().is_some_and(|summary| {
                summary.len() <= 5
                    && summary.iter().any(|line| {
                        line.as_str()
                            .is_some_and(|line| line.contains("INV-ROUTING"))
                    })
                    && summary
                        .iter()
                        .any(|line| line.as_str().is_some_and(|line| line.contains("Decision:")))
            })
        }) {
            failures.push(format!(
                "{stage}/{seam}: governing ADR invariants must be inlined"
            ));
        }
        let callers = bundle["external_callers"].as_array().unwrap();
        if callers
            .iter()
            .any(|caller| caller["path"] == "tests/external.rs")
        {
            failures.push(format!(
                "{stage}/{seam}: external boundary context must omit test callers"
            ));
        }
        if !admit_caller
            && ![
                "src/caller.rs",
                "src/possible.rs",
                "src/implements.rs",
                "src/overrides.rs",
            ]
            .iter()
            .all(|path| callers.iter().any(|caller| caller["path"] == *path))
        {
            failures.push(format!(
                "{stage}/{seam}: boundary context must include callers across all indexed edge kinds"
            ));
        }
        if !bundle["covering_tests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|test| test["path"] == "src/caller.rs")
        {
            failures.push(format!(
                "{stage}/{seam}: covering tests must follow two-hop production calls"
            ));
        }
        tasks::blackboard(&root.0, serde_json::from_value(json!({"action":"post","task_id":task_id,"topic":"draft","gate":"build","payload":"Keep route_task callers in scope when changing its signature; use tests/connected.rs."})).unwrap(), "contract-author").unwrap();
        if stage == "build" {
            let task = tasks::store_for_task(&root.0, task_id)
                .unwrap()
                .inspect(task_id)
                .unwrap();
            let mut proof = evidence(&task);
            proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
            tasks::submit(
                &root.0,
                tasks::Submit {
                    task_id: task_id.into(),
                    stage: "contract".into(),
                    action: tasks::Action::Pass,
                    evidence: serde_json::to_value(proof).unwrap(),
                    findings: None,
                },
            )
            .unwrap();
            claim = tasks::claim(&root.0, serde_json::from_value(json!({"task_id":task_id,"stage":"build","worker_id":"builder","worktree":root.0})).unwrap()).unwrap();
            if !claim["workflow_guidance"]["blackboard_messages"]
                .as_array()
                .is_some_and(|messages| {
                    messages
                        .iter()
                        .any(|message| message["gate"] == "build" && message["topic"] == "draft")
                })
            {
                failures.push(format!(
                    "{stage}/{seam}: builder must receive the contract author's targeted handoff"
                ));
            }
            let scoped = &claim["workflow_guidance"]["scoped_commands"];
            let scoped_command = |name: &str, path: &str| {
                profile
                    .commands
                    .get(name)
                    .and_then(|definition| definition.command_for_path(path))
                    .map(|command| {
                        let scope_target = profile
                            .scope_args_for_path(path)
                            .unwrap_or_else(|| path.to_owned());
                        if command.contains("{scope_target}") {
                            command.replace("{scope_target}", &scope_target)
                        } else {
                            format!("{command} {scope_target}")
                        }
                    })
            };
            let expected_targets = [
                "src/caller.rs",
                "src/routing.rs",
                "tests/acdd/tasks.rs",
                "tests/connected.rs",
                "tests/external.rs",
            ];
            if scoped.is_null()
                || !scoped.as_array().is_some_and(|entries| {
                    !entries.is_empty()
                        && entries.iter().all(|entry| {
                            let Some(name) = entry["name"].as_str() else {
                                return false;
                            };
                            let Some(path) = entry["test_path"].as_str() else {
                                return false;
                            };
                            scoped_command(name, path)
                                .is_some_and(|expected| entry["command"] == expected)
                        })
                        && expected_targets.iter().all(|path| {
                            profile.commands.iter().all(|(name, definition)| {
                                let Some(command) = definition.command_for_path(path) else {
                                    return true;
                                };
                                let scope_target = profile
                                    .scope_args_for_path(path)
                                    .unwrap_or_else(|| (*path).to_owned());
                                let expected = if command.contains("{scope_target}") {
                                    command.replace("{scope_target}", &scope_target)
                                } else {
                                    format!("{command} {scope_target}")
                                };
                                entries.iter().any(|entry| {
                                    entry["name"] == name.as_str()
                                        && entry["test_path"] == *path
                                        && entry["command"] == expected
                                })
                            })
                        })
                })
            {
                failures.push(format!("{stage}/{seam}: scoped commands must include every configured command for each matching seam and covering-test path: {scoped}"));
            }
        }
        if change_signature {
            root.write(
                "src/routing.rs",
                "pub fn route_task(value: usize) -> usize { value + 1 }\npub fn test_only_route(value: usize) -> usize { value + 3 }\n#[cfg(test)]\nmod tests {\n    use crate::routing::route_task;\n    #[test]\n    fn test_connected() {\n        let actual = route_task(0);\n        assert_eq!(actual, 1);\n    }\n}\n",
            );
            root.write("tests/connected.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
            root.write("tests/external.rs", "use tiered_fixture::routing::test_only_route;\n#[test]\nfn external_test_only_caller() { let actual = test_only_route(0); assert_eq!(actual, 3); }\n");
            if admit_caller {
                root.write("src/caller.rs", "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task(0) }\n#[cfg(test)]\nmod tests {\n    use crate::caller::invoke_route;\n    #[test]\n    fn test_indirect() {\n        let actual = invoke_route();\n        assert_eq!(actual, 1);\n    }\n}\n");
            }
        }
        let store = tasks::store_for_task(&root.0, task_id).unwrap();
        let task = store.inspect(task_id).unwrap();
        let mut proof = evidence(&task);
        if stage == "contract" {
            proof.proof = json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}});
        } else {
            let command = profile
                .commands
                .values()
                .find(|command| command.command() == canonical_command)
                .or_else(|| profile.commands.values().next())
                .unwrap()
                .command();
            proof.proof = json!({"command_proof":{"command":format!("{command} --test connected"),"exit_code":0,"tests_passed":1,"tests_failed":0}});
        }
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: stage.into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(proof).unwrap(),
                findings: None,
            },
        );
        match (expected_error, result) {
            (Some(code), Err(error)) => {
                assert!(error.to_string().contains(code), "{stage}/{seam}: {error}");
                if change_signature && !admit_caller {
                    for path in [
                        "src/caller.rs",
                        "src/possible.rs",
                        "src/implements.rs",
                        "src/overrides.rs",
                    ] {
                        assert!(
                            error.to_string().contains(path),
                            "{stage}/{seam}: signature gate omitted {path}: {error}"
                        );
                    }
                    assert!(
                        !error.to_string().contains("tests/external.rs"),
                        "test callers must not cause boundary rejection: {error}"
                    );
                }
                assert_eq!(
                    store.inspect(task_id).unwrap().stage,
                    stage,
                    "a rejected boundary must preserve the active gate"
                );
            }
            (Some(code), Ok(_)) => {
                failures.push(format!("{stage}/{seam}: must reject with {code}"))
            }
            (None, Err(error)) => failures.push(format!(
                "{stage}/{seam}: admitted caller signature change must pass: {error}"
            )),
            (None, Ok(_)) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn seam_scope_directory_prefix_treats_sql_wildcards_as_literal() {
    for (case, scope_dir, actual_dir) in [
        ("underscore", "api_v1", "apiXv1"),
        ("percent", "api%v1", "apiXYZv1"),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_tiered_literal_scope_{case}"));
        root.write("src/lib.rs", &format!("pub mod {actual_dir};\n"));
        root.write(
            &format!("src/{scope_dir}/allowed.rs"),
            "pub fn placeholder() {}\n",
        );
        root.write(&format!("src/{actual_dir}/mod.rs"), "pub mod hidden;\n");
        root.write(
            &format!("src/{actual_dir}/hidden.rs"),
            &format!(
                "pub fn route_task() -> usize {{ 1 }}\n#[cfg(test)]\nmod tests {{ use crate::{actual_dir}::hidden::route_task; #[test] fn test_connected() {{ let actual = route_task(); assert_eq!(actual, 1); }} }}\n"
            ),
        );
        root.write(
            "Cargo.toml",
            "[package]\nname = \"tiered_literal_scope_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
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
        let milestone_ref = format!("docs/milestones/010-tiered-literal-{case}.md");
        let milestone_id = format!("m-tiered-literal-{case}");
        let task_ref = format!("literal-scope-{case}");
        root.write(
            &milestone_ref,
            &format!(
                "---\nid: {milestone_id}\ntitle: Literal directory scope\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: {task_ref}\ntarget: Keep directory scope matching literal\nproof_policy: seam-test-first\nscope: [src/{scope_dir}/, tests/acdd/tasks.rs]\nscope_roots: [src/, tests/]\n```\n"
            ),
        );
        commit_fixture_baseline(&root);
        writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref})).unwrap(),
        )
        .unwrap();
        let task_id = format!("forge-mcp/{milestone_id}:{task_ref}");
        tasks::claim(
            &root.0,
            serde_json::from_value(json!({"task_id":task_id,"stage":"contract","worker_id":"contract-author","worktree":root.0})).unwrap(),
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, &task_id)
            .unwrap()
            .inspect(&task_id)
            .unwrap();
        let mut contract = evidence(&task);
        contract.proof = json!({"contract_proof":{"seam_test_ref":format!("src/{actual_dir}/hidden.rs::test_connected"),"red_exit_code":101}});
        let result = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.clone(),
                stage: "contract".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(contract).unwrap(),
                findings: None,
            },
        );
        let error = result.expect_err(&format!(
            "src/{actual_dir}/ must not match literal directory src/{scope_dir}/"
        ));
        assert!(
            error.to_string().contains("TASK_SEAM_DISCONNECTED"),
            "{case}: {error}"
        );
    }
}

#[test]
fn greenfield_seam_diagnostic_reports_missing_scope_files_and_scaffolding_guidance() {
    let root = prepare_indexed_boundary_workspace(
        "pub fn route() -> usize { 1 }\n",
        "",
        "use crate::api::route;\n#[test]\nfn test_connected() { let actual = route(); assert_eq!(actual, 1); }\n",
        "route",
        None,
        None,
    );
    std::fs::remove_file(root.0.join("src/api.rs")).unwrap();
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let task_id = "forge-mcp/m-current-boundary:routing";
    tasks::claim(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "contract",
            "worker_id": "contract-author",
            "worktree": root.0
        }))
        .unwrap(),
    )
    .unwrap();
    let mut store = tasks::store_for_task(&root.0, task_id).unwrap();
    let claimed = store.inspect(task_id).unwrap();
    let mut proof = evidence(&claimed);
    proof.proof = json!({
        "contract_proof": {
            "seam_test_ref": "src/seam.rs::test_connected",
            "red_exit_code": 101
        }
    });

    for phase in ["missing", "stub_unindexed", "stub_indexed"] {
        if phase == "stub_unindexed" {
            root.write("src/api.rs", "pub fn route() -> usize { 0 }\n");
        } else if phase == "stub_indexed" {
            writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
        }
        let result = store.submit(task_id, "contract", &proof, "pass", None);
        if phase == "stub_indexed" {
            result.expect("indexed real production stub must permit red contract proof");
            assert_eq!(store.inspect(task_id).unwrap().stage, "build");
            continue;
        }
        let error = result.unwrap_err().to_string();
        assert!(error.contains("TASK_SEAM_DISCONNECTED"), "{phase}: {error}");
        if phase == "missing" {
            assert!(
                error.contains("target scope files do not exist yet on disk (src/api.rs)"),
                "{error}"
            );
            assert!(error.contains("create minimal file scaffolding"), "{error}");
            assert!(error.contains("contextunity-forge-mcp build"), "{error}");
        } else {
            assert!(!error.contains("do not exist yet"), "{error}");
            assert!(
                error.contains("refresh the claimed-worktree code map"),
                "{error}"
            );
        }
        let rejected = store.inspect(task_id).unwrap();
        assert_eq!(rejected.stage, "contract", "{phase}");
        assert_eq!(rejected.claim_revision, claimed.claim_revision, "{phase}");
    }
}
