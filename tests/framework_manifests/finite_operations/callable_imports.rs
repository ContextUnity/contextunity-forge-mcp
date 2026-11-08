use super::*;
use serde_json::json;

#[test]
fn callable_imports_preserve_exact_targets_across_provider_deltas() {
    let mut failures = Vec::new();
    let workspace = FrameworkHarness::persisted(&[]);
    let mut indexed = false;
    enum ExpectedCold<'a> {
        Filesystem,
        SourceInventory(&'a DependencyRegistry),
    }
    // The same persisted seam proves arbitrary descriptor spellings, actual
    // callable destinations and provider-only replay without a factory result.
    for renamed in [false, true] {
        let cold_references = ColdFilesystemReferenceCache::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let rewrite = |text: &str| {
                [
                    ("cove-runtime", "shore-runtime"),
                    ("declareCove", "prepareShore"),
                    ("loading", "lookup"),
                    ("automatic", "enabled"),
                    ("discover", "scanNow"),
                    ("sourceRoot", "rootOverride"),
                    ("providers", "exports"),
                    ("readParcel", "loadCrate"),
                    ("~/", "!/"),
                    ("@/", "=/"),
                ]
                .into_iter()
                .fold(text.to_owned(), |value, (from, to)| {
                    if renamed {
                        value.replace(from, to)
                    } else {
                        value
                    }
                })
            };
            let manifest = ".forge/frameworks/callables.toml";
            let configuration = "app.config.ts";
            let provider = "providers/parcel.ts";
            let provider_source = "export function readParcel() { return 1; }\n";
            let configured = "export default declareCove({});\n";
            let package = r#"{"dependencies":{"cove-runtime":"1"}}"#;
            let policy = pack(
                vec![],
                vec![syntax(
                    "callables",
                    "typescript",
                    json!({
                        "kind":"component_environment", "package":"cove-runtime",
                        "config_file":"app.config.ts", "config_callee":"declareCove",
                        "enabled_key":"widgets", "modules_key":"extensions",
                        "directory":"widgets", "extension":".vue", "virtual_module":"#widgets",
                        "autoimports":[], "builtins":[], "modules":{},
                        "callable_imports": {
                            "exports":[{"directory":"providers","extension":".ts"}],
                            "aliases":["~/","@/"], "options_key":"loading",
                            "enabled_key":"automatic", "scan_key":"discover",
                            "unsupported_keys":["sourceRoot","loading.paths"]
                        }
                    }),
                )],
            );
            let policy_source = rewrite(&toml::to_string(&policy).unwrap());
            let mut baseline_paths = vec!["package.json", configuration, manifest];
            workspace.write("package.json", &rewrite(package));
            workspace.write(configuration, &rewrite(configured));
            workspace.write(manifest, &policy_source);
            for (path, source) in [
                (
                    "client.ts",
                    "export function invoke() { return readParcel(); }\n",
                ),
                (
                    "alias.ts",
                    "import {readParcel as chosen} from '~/providers/parcel';\ntype Selected = ReturnType<typeof chosen>;\nexport function invoke() { return chosen(); }\n",
                ),
                (
                    "second-alias.ts",
                    "import {readParcel as second} from '@/providers/parcel';\nexport function invoke() { return second(); }\n",
                ),
                (
                    "relative.ts",
                    "import {readParcel as direct} from './providers/parcel';\ntype Selected = ReturnType<typeof direct>;\nexport function invoke() { return direct(); }\n",
                ),
                (
                    "local.ts",
                    "function readParcel() { return 2; }\nexport function invoke() { return readParcel(); }\n",
                ),
                (
                    "parameter.ts",
                    "export function invoke(readParcel) { return readParcel(); }\n",
                ),
                (
                    "foreign.ts",
                    "import {readParcel} from 'foreign-kit';\nexport function invoke() { return readParcel(); }\n",
                ),
                (
                    "rebound.ts",
                    "import {readParcel as chosen} from '~/providers/parcel';\nchosen = replacement;\nexport function invoke() { return chosen(); }\n",
                ),
                (
                    "member.ts",
                    "class Service { readParcel() { return 2; } invoke() { return this.readParcel(); } }\nconst owner = {readParcel() { return 3; }};\nexport function invoke() { return owner.readParcel(); }\n",
                ),
                (
                    "written.ts",
                    "let readParcel = replacement;\nexport function invoke() { readParcel = another; return readParcel(); }\n",
                ),
                ("plain/package.json", r#"{"dependencies":{}}"#),
                (
                    "plain/client.ts",
                    "export function invoke() { return readParcel(); }\n",
                ),
            ] {
                workspace.write(path, &rewrite(source));
                baseline_paths.push(path);
            }
            if indexed {
                for stale in [
                    "providers/parcel.ts",
                    "providers/duplicate.ts",
                    "exports/parcel.ts",
                    "exports/duplicate.ts",
                ] {
                    if workspace.path(stale).exists() {
                        workspace.remove(stale);
                    }
                }
                baseline_paths.extend([
                    "providers/parcel.ts",
                    "providers/duplicate.ts",
                    "exports/parcel.ts",
                    "exports/duplicate.ts",
                ]);
                workspace.delta(&baseline_paths);
            } else {
                workspace.build();
                indexed = true;
            }
            let phase_failures = std::cell::RefCell::new(Vec::new());
            let check = |phase: &str,
                         implicit: &str,
                         alias: bool,
                         relative: bool,
                         target: &str,
                         expected: ExpectedCold<'_>| {
                let observation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let conn = workspace.open();
                    commitments::verify(&conn).unwrap();
                    assert!(error_messages(&conn).is_empty(), "{phase}");
                    for (path, line, expression, kind, expected, exact) in [
                        (
                            "client.ts",
                            1,
                            "readParcel",
                            "calls",
                            implicit,
                            implicit == "resolved",
                        ),
                        (
                            "alias.ts",
                            2,
                            "chosen",
                            "references",
                            if alias { "resolved" } else { "nonexact" },
                            alias,
                        ),
                        (
                            "alias.ts",
                            3,
                            "chosen",
                            "calls",
                            if alias { "resolved" } else { "nonexact" },
                            alias,
                        ),
                        (
                            "second-alias.ts",
                            2,
                            "second",
                            "calls",
                            if alias { "resolved" } else { "nonexact" },
                            alias,
                        ),
                        (
                            "relative.ts",
                            2,
                            "direct",
                            "references",
                            if relative { "resolved" } else { "nonexact" },
                            relative,
                        ),
                        (
                            "relative.ts",
                            3,
                            "direct",
                            "calls",
                            if relative { "resolved" } else { "nonexact" },
                            relative,
                        ),
                    ] {
                        let states = coverage_statuses(&conn, path, line, &rewrite(expression));
                        if expected == "nonexact" {
                            assert!(
                                !states.is_empty()
                                    && states.iter().all(|s| matches!(
                                        s.as_str(),
                                        "unresolved" | "ambiguous"
                                    ) || (path != "client.ts"
                                        && s == "external")),
                                "{phase} renamed={renamed}: {path}:{line} {states:?}"
                            );
                        } else {
                            assert_eq!(
                                states,
                                [expected],
                                "{phase} renamed={renamed}: {path}:{line}"
                            );
                        }
                        let destinations = operations::targets_on_line(&conn, path, line, kind);
                        let canonical = if exact {
                            vec![node_id_by_kind(
                                &conn,
                                &rewrite(provider),
                                "function",
                                &rewrite(target),
                            )]
                        } else {
                            vec![]
                        };
                        if exact {
                            assert_eq!(canonical.len(), 1, "{phase}: actual callable Node");
                        }
                        assert_eq!(
                            destinations, canonical,
                            "{phase} renamed={renamed}: {path}:{line} exact target"
                        );
                    }
                    for path in [
                        "local.ts",
                        "parameter.ts",
                        "foreign.ts",
                        "rebound.ts",
                        "member.ts",
                        "written.ts",
                        "plain/client.ts",
                    ] {
                        let count = provider_edge_count(
                            &conn,
                            path,
                            &format!("{}/%", rewrite("providers")),
                        );
                        assert_eq!(count, 0, "{phase}: lexical/project precedence {path}");
                    }
                    assert_eq!(
                        coverage_statuses_for_path(&conn, "local.ts", &rewrite("readParcel")),
                        ["resolved"],
                        "{phase}: real local callable stays resolved"
                    );
                    let (reopened, cold) = match expected {
                        ExpectedCold::Filesystem => operations::check_cold(
                            cold_references.observe(&workspace, conn, persisted_rows),
                            "filesystem cold",
                        ),
                        ExpectedCold::SourceInventory(registry) => {
                            let adapter = contextunity_forge_mcp::engine::scanner::load_adapter(
                                workspace.root(),
                                None,
                            )
                            .unwrap();
                            assert!(
                                adapter.linked_workspaces.is_empty(),
                                "RAM fixture inventory requires a single workspace"
                            );
                            let report =
                                contextunity_forge_mcp::engine::scanner::scan_with_adapter(
                                    workspace.root(),
                                    &adapter,
                                )
                                .unwrap();
                            let mut sources = VirtualWorkspace::default();
                            for file in report.entries {
                                let source =
                                    std::fs::read_to_string(workspace.path(&file.path)).unwrap();
                                assert_eq!(
                                    commitments::hash(source.as_bytes()),
                                    file.digest,
                                    "fixture changed after scanner admission: {}",
                                    file.path
                                );
                                sources.write(&file.path, &source);
                            }
                            for dependency_manifest in report.dependency_manifests {
                                let relative = dependency_manifest
                                    .strip_prefix(workspace.root())
                                    .unwrap()
                                    .to_str()
                                    .unwrap();
                                sources.write(
                                    relative,
                                    &std::fs::read_to_string(&dependency_manifest).unwrap(),
                                );
                            }
                            sources.write(
                                manifest,
                                &std::fs::read_to_string(workspace.path(manifest)).unwrap(),
                            );
                            let cold = sources.index_with_registry(registry).unwrap();
                            operations::check_cold(
                                ColdObservations::from_connections(conn, cold, persisted_rows),
                                "source inventory semantic recomputation",
                            )
                        }
                    };
                    let actual_rows = callable_rows(&reopened);
                    let expected_rows = callable_rows(&cold);
                    assert_eq!(
                        actual_rows.coverage, expected_rows.coverage,
                        "{phase}: reopened cold/delta identity: complete coverage"
                    );
                    assert_eq!(
                        actual_rows.occurrences, expected_rows.occurrences,
                        "{phase}: reopened cold/delta identity: complete edge occurrences"
                    );
                    assert_eq!(
                        all_local_facts(&reopened),
                        all_local_facts(&cold),
                        "{phase}: complete persisted facts"
                    );
                }));
                if let Err(payload) = observation {
                    let message = payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("non-string panic");
                    phase_failures
                        .borrow_mut()
                        .push(format!("{phase}: {message}"));
                }
            };
            macro_rules! check_filesystem {
                ($phase:expr, $implicit:expr, $alias:expr, $relative:expr) => {
                    check_filesystem!($phase, $implicit, $alias, $relative, "readParcel")
                };
                ($phase:expr, $implicit:expr, $alias:expr, $relative:expr, $target:expr) => {
                    check(
                        $phase,
                        $implicit,
                        $alias,
                        $relative,
                        $target,
                        ExpectedCold::Filesystem,
                    )
                };
            }
            check_filesystem!("missing provider", "unresolved", false, false);
            workspace.write(rewrite(provider), &rewrite(provider_source));
            workspace.delta(&[&rewrite(provider)]);
            check_filesystem!("provider-only addition", "resolved", true, true);
            for (label, source, implicit, aliases, relative, target) in [
                (
                    "export renamed",
                    "export function otherParcel() { return 1; }\n",
                    "unresolved",
                    false,
                    false,
                    "readParcel",
                ),
                (
                    "unknown export",
                    "export {readParcel} from './missing';\n",
                    "unresolved",
                    false,
                    false,
                    "readParcel",
                ),
                (
                    "local exported alias",
                    "function actualParcel() { return 1; }\nexport {actualParcel as readParcel};\n",
                    "resolved",
                    true,
                    true,
                    "actualParcel",
                ),
                (
                    "provider-only alias retarget",
                    "function alternateParcel() { return 2; }\nexport {alternateParcel as readParcel};\n",
                    "resolved",
                    true,
                    true,
                    "alternateParcel",
                ),
                (
                    "restore direct export",
                    provider_source,
                    "resolved",
                    true,
                    true,
                    "readParcel",
                ),
            ] {
                workspace.write(rewrite(provider), &rewrite(source));
                workspace.delta(&[&rewrite(provider)]);
                check_filesystem!(label, implicit, aliases, relative, target);
            }
            workspace.write(rewrite("providers/duplicate.ts"), &rewrite(provider_source));
            workspace.delta(&[&rewrite("providers/duplicate.ts")]);
            check_filesystem!("distinct complete duplicate", "ambiguous", true, true);
            workspace.write(
                rewrite("providers/duplicate.ts"),
                &rewrite("export {readParcel} from './missing';\n"),
            );
            workspace.delta(&[&rewrite("providers/duplicate.ts")]);
            check_filesystem!("known plus unknown competitor", "nonexact", true, true);
            let duplicate = rewrite("providers/duplicate.ts");
            workspace.remove(&duplicate);
            workspace.delta(&[&duplicate]);
            check_filesystem!("duplicate removed", "resolved", true, true);
            let configuration_registry = FrameworkHarness::from_manifests(&[
                ("package.json", &rewrite(package)),
                ("plain/package.json", r#"{"dependencies":{}}"#),
                (manifest, &policy_source),
            ])
            .unwrap();
            assert_eq!(
                configuration_registry.registry.digest(),
                DependencyRegistry::collect_with_scan_config(workspace.root(), None).digest(),
                "unchanged package and policy inputs compile the same immutable registry"
            );
            for (label, source, implicit, aliases) in [
                (
                    "implicit disabled",
                    "export default declareCove({loading:{automatic:false}});\n",
                    "unresolved",
                    true,
                ),
                (
                    "scan disabled",
                    "export default declareCove({loading:{discover:false}});\n",
                    "unresolved",
                    true,
                ),
                (
                    "component flag independent",
                    "export default declareCove({widgets:false});\n",
                    "resolved",
                    true,
                ),
                (
                    "dynamic options",
                    "export default declareCove({loading:chooseOptions()});\n",
                    "unresolved",
                    false,
                ),
                (
                    "dynamic flag",
                    "export default declareCove({loading:{automatic:enabledNow}});\n",
                    "unresolved",
                    false,
                ),
                (
                    "duplicate flag",
                    "export default declareCove({loading:{automatic:true,automatic:false}});\n",
                    "unresolved",
                    false,
                ),
                (
                    "spread configuration",
                    "export default declareCove({...settings});\n",
                    "unresolved",
                    false,
                ),
                (
                    "unsupported scope override",
                    "export default declareCove({sourceRoot:'elsewhere'});\n",
                    "unresolved",
                    false,
                ),
                (
                    "unsupported provider override",
                    "export default declareCove({loading:{paths:['elsewhere']}});\n",
                    "unresolved",
                    false,
                ),
                (
                    "unproved callee",
                    "export default otherConfiguration({});\n",
                    "unresolved",
                    false,
                ),
                ("config restored", configured, "resolved", true),
            ] {
                workspace.write(configuration, &rewrite(source));
                workspace.delta(&[configuration]);
                check(
                    label,
                    implicit,
                    aliases,
                    true,
                    "readParcel",
                    ExpectedCold::SourceInventory(&configuration_registry.registry),
                );
            }
            let mut absent_descriptor = policy.clone();
            absent_descriptor["syntax_rules"][0]["operation"]
                .as_object_mut()
                .unwrap()
                .remove("callable_imports");
            workspace.write(
                manifest,
                &rewrite(&toml::to_string(&absent_descriptor).unwrap()),
            );
            workspace.delta(&[manifest]);
            check_filesystem!("descriptor absent", "unresolved", false, true);
            workspace.write(manifest, &policy_source);
            workspace.delta(&[manifest]);
            check_filesystem!("descriptor restored", "resolved", true, true);
            workspace.write("package.json", r#"{"dependencies":{}}"#);
            workspace.delta(&["package.json"]);
            check_filesystem!("package removed", "unresolved", false, true);
            workspace.write("package.json", &rewrite(package));
            workspace.delta(&["package.json"]);
            check_filesystem!("package restored", "resolved", true, true);
            workspace.remove(configuration);
            workspace.delta(&[configuration]);
            check_filesystem!("config removed", "unresolved", false, true);
            workspace.write(configuration, &rewrite(configured));
            workspace.delta(&[configuration]);
            check_filesystem!("config re-added", "resolved", true, true);
            let provider_path = rewrite(provider);
            workspace.remove(&provider_path);
            workspace.delta(&[&provider_path]);
            check_filesystem!("provider removed", "unresolved", false, false);
            workspace.write(rewrite(provider), &rewrite(provider_source));
            workspace.delta(&[&rewrite(provider)]);
            check_filesystem!("provider restored", "resolved", true, true);
            workspace.remove(manifest);
            workspace.delta(&[manifest]);
            check_filesystem!("rule removed", "unresolved", false, true);
            workspace.write(manifest, &policy_source);
            workspace.delta(&[manifest]);
            check_filesystem!("rule restored", "resolved", true, true);
            let before = publication_commitments(&workspace.open());
            for invalid in [
                json!({"unchecked":true}),
                json!({"aliases":["~/","~/"]}),
                json!({"aliases":(0..9).map(|index|format!("scope{index}/")).collect::<Vec<_>>()}),
                json!({"exports":[{"directory":"../providers","extension":".ts"}]}),
                json!({"unsupported_keys":["one.two.three.four.five"]}),
                json!({"unsupported_keys":(0..17).map(|index|format!("key{index}")).collect::<Vec<_>>()}),
                json!({"options_key":"x".repeat(129)}),
            ] {
                let mut rejected = policy.clone();
                rejected["syntax_rules"][0]["operation"]["callable_imports"]
                    .as_object_mut()
                    .unwrap()
                    .extend(invalid.as_object().unwrap().clone());
                workspace.write(manifest, &rewrite(&toml::to_string(&rejected).unwrap()));
                assert!(
                    contextunity_forge_mcp::engine::pipeline::build_with_cache_dir(
                        workspace.root(),
                        &workspace.db(),
                        None,
                        &workspace.path(".forge/extraction-cache"),
                    )
                    .is_err(),
                    "renamed={renamed}: strict descriptor boundary"
                );
                commitments::verify(&workspace.open()).unwrap();
                assert_eq!(
                    publication_commitments(&workspace.open()),
                    before,
                    "rejected descriptor cannot publish"
                );
            }
            workspace.write(manifest, &policy_source);
            assert!(
                phase_failures.borrow().is_empty(),
                "{}",
                phase_failures.borrow().join("\n")
            );
        }));
        if let Err(payload) = result {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            failures.push(format!("callable imports renamed={renamed}: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
