use super::*;

#[test]
fn tiered_task_context_enforces_indexed_seams_and_signature_boundaries() {
    assert_safe_git_environment();
    let mut failures = Vec::new();
    let mut fixtures: HashMap<(&str, bool), (Workspace, FixtureSnapshot)> = HashMap::new();
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
        let contract_seam = if stage == "build" {
            "src/routing.rs::test_connected"
        } else {
            seam
        };
        let fixture = fixtures.entry((contract_seam, admit_caller)).or_insert_with(|| {
        let root = acdd_workspace(
            contract_seam,
            r#"scope_args_by_pattern:
  "src/main.rs": "--bin contextunity-forge-mcp"
  "src/**/*.rs": "--lib"
  "tests/*.rs": "--test {file_stem}"
  "tests/acdd/**/*.rs": "--test acdd"
"#,
        );
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
        root.write("tests/acdd.rs", "#[path = \"acdd/tasks.rs\"]\nmod tasks;\n");
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
        root.write("docs/adr/001-routing.md", "---\ntitle: Routing invariants\ndoc_type: adr\nstatus: accepted\n---\n# Routing invariants\n- INV-ROUTING: [`src.routing.route_task`](../../src/routing.rs#L1) remains observable through the public seam.\n- Decision: Admit every caller before changing the public signature.\n");
        let caller_scope = if admit_caller {
            ", src/caller.rs, src/possible.rs, src/implements.rs, src/overrides.rs"
        } else {
            ""
        };
        let specification = format!("---\nid: m-tiered\ntitle: Tiered boundary gates\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: routing\ntarget: Preserve the routing boundary\nproof_policy: seam-test-first\nscope: [src/routing.rs, tests/connected.rs, tests/acdd/tasks.rs{caller_scope}]\nscope_roots: [src/, tests/]\n```\n");
        root.write("docs/milestones/010-tiered.md", &specification);
        commit_fixture_baseline(&root);
        root.build_to(root.path(".forge/code-map.sqlite"));
        let connection = rusqlite::Connection::open(root.path(".forge/code-map.sqlite")).unwrap();
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
        tasks::manage(
            root.root(),
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-tiered.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let snapshot = FixtureSnapshot::capture(&root);
        (root, snapshot)
        });
        fixture.1.restore(&fixture.0);
        let root = &fixture.0;
        let task_id = "forge-mcp/m-tiered:routing";
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(root.root()).unwrap();
        let defaults = contextunity_forge_mcp::core::tasks::profile::compiled();
        if !defaults.commands.values().any(|command| {
            command.command()
                == "cargo clippy --all-targets --all-features -- -D warnings && cargo test"
        }) {
            failures.push(format!("{stage}/{seam}: default Rust profile must register static checks before cargo test"));
        }
        if !defaults
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
        if ![
            "contract_author",
            "builder",
            "independent_reviewer",
            "architect",
        ]
        .iter()
        .all(|r| profile.roles.contains_key(*r))
        {
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
        let mut claim =
            TaskHarness::claim_at(root.root(), task_id, "contract", "contract-author").unwrap();
        let guidance = &claim["workflow_guidance"];
        if !guidance["rules"].as_array().unwrap().iter().any(|rule| {
            rule.as_str()
                .is_some_and(|rule| rule.starts_with("NO_PREFLIGHT_SUBAGENTS:"))
        }) {
            failures.push(format!(
                "{stage}/{seam}: task guidance must prohibit preflight subagents"
            ));
        }
        let has_build_handoff = guidance["role_guide"].as_str().is_some_and(|guide| {
            guide.contains("build-targeted draft") || guide.contains("builder")
        }) || guidance["steps"].as_array().is_some_and(|steps| {
            steps.iter().any(|step| {
                step.as_str().is_some_and(|step| {
                    step.contains("gate=\"build\"") || step.contains("gate=build")
                })
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
        tasks::blackboard(root.root(), serde_json::from_value(json!({"action":"post","task_id":task_id,"topic":"draft","gate":"build","payload":"Keep route_task callers in scope when changing its signature; use tests/connected.rs."})).unwrap(), "contract-author").unwrap();
        if stage == "build" {
            let task = tasks::store_for_task(root.root(), task_id)
                .unwrap()
                .inspect(task_id)
                .unwrap();
            let mut proof = TaskHarness::default_evidence(&task);
            proof.proof = json!({"contract_proof":{"seam_test_ref":"src/routing.rs::test_connected","red_exit_code":101}});
            tasks::submit(
                root.root(),
                TaskHarness::pass_request(task_id, "contract", &proof),
            )
            .unwrap();
            claim = TaskHarness::claim_at(root.root(), task_id, "build", "builder").unwrap();
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
            root.write("tests/acdd/tasks.rs", "use tiered_fixture::routing::route_task;\n#[test]\nfn test_nested_connected() { let actual = route_task(0); assert_eq!(actual, 1); }\n");
            root.write("tests/external.rs", "use tiered_fixture::routing::test_only_route;\n#[test]\nfn external_test_only_caller() { let actual = test_only_route(0); assert_eq!(actual, 3); }\n");
            if admit_caller {
                root.write("src/caller.rs", "use crate::routing::route_task;\npub fn invoke_route() -> usize { route_task(0) }\n#[cfg(test)]\nmod tests {\n    use crate::caller::invoke_route;\n    #[test]\n    fn test_indirect() {\n        let actual = invoke_route();\n        assert_eq!(actual, 1);\n    }\n}\n");
            }
        }
        let store = tasks::store_for_task(root.root(), task_id).unwrap();
        let task = store.inspect(task_id).unwrap();
        let mut proof = TaskHarness::default_evidence(&task);
        if stage == "contract" {
            proof.proof = json!({"contract_proof":{"seam_test_ref":seam,"red_exit_code":101}});
        }
        let result = tasks::submit(
            root.root(),
            TaskHarness::pass_request(task_id, stage, &proof),
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
