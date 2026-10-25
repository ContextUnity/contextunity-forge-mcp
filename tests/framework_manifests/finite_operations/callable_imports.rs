use super::*;
use serde_json::json;

pub(super) fn run(failures: &mut Vec<String>) {
    // The same persisted seam proves arbitrary descriptor spellings, actual
    // callable destinations and provider-only replay without a factory result.
    for renamed in [false, true] {
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
            let workspace = Workspace::new();
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
            workspace.write("package.json", &rewrite(package));
            workspace.write(configuration, &rewrite(configured));
            workspace.write(manifest, &policy_source);
            for (path, source) in [
                ("client.ts", "export function invoke() { return readParcel(); }\n"),
                ("alias.ts", "import {readParcel as chosen} from '~/providers/parcel';\ntype Selected = ReturnType<typeof chosen>;\nexport function invoke() { return chosen(); }\n"),
                ("second-alias.ts", "import {readParcel as second} from '@/providers/parcel';\nexport function invoke() { return second(); }\n"),
                ("relative.ts", "import {readParcel as direct} from './providers/parcel';\ntype Selected = ReturnType<typeof direct>;\nexport function invoke() { return direct(); }\n"),
                ("local.ts", "function readParcel() { return 2; }\nexport function invoke() { return readParcel(); }\n"),
                ("parameter.ts", "export function invoke(readParcel) { return readParcel(); }\n"),
                ("foreign.ts", "import {readParcel} from 'foreign-kit';\nexport function invoke() { return readParcel(); }\n"),
                ("rebound.ts", "import {readParcel as chosen} from '~/providers/parcel';\nchosen = replacement;\nexport function invoke() { return chosen(); }\n"),
                ("member.ts", "class Service { readParcel() { return 2; } invoke() { return this.readParcel(); } }\nconst owner = {readParcel() { return 3; }};\nexport function invoke() { return owner.readParcel(); }\n"),
                ("written.ts", "let readParcel = replacement;\nexport function invoke() { readParcel = another; return readParcel(); }\n"),
                ("plain/package.json", r#"{"dependencies":{}}"#),
                ("plain/client.ts", "export function invoke() { return readParcel(); }\n"),
            ] {
                workspace.write(path, &rewrite(source));
            }
            writer::build(&workspace.0, &workspace.db(), None).unwrap();
            let phase_failures = std::cell::RefCell::new(Vec::new());
            let check = |phase: &str, implicit: &str, alias: bool, relative: bool, target: &str| {
                let observation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let conn = workspace.open();
                    commitments::verify(&conn).unwrap();
                    assert!(
                        query_rows(&conn, "SELECT message FROM errors").is_empty(),
                        "{phase}"
                    );
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
                        let destinations = query_rows_params(&conn, "SELECT d.id FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes d ON d.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind=?3 ORDER BY d.id", params![path, line, kind]);
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
                        let count = query_count_usize(&conn, "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE p.path=?1 AND dp.path LIKE ?2", params![path, format!("{}/%", rewrite("providers"))]);
                        assert_eq!(count, 0, "{phase}: lexical/project precedence {path}");
                    }
                    assert_eq!(
                        coverage_statuses_for_path(&conn, "local.ts", &rewrite("readParcel")),
                        ["resolved"],
                        "{phase}: real local callable stays resolved"
                    );
                    drop(conn);
                    assert_delta_cold_parity(&workspace);
                    let reopened = workspace.open();
                    let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0)
                        .unwrap();
                    for sql in [
                    "SELECT json_array(p.path,c.line,x.expression,c.status) FROM resolution_coverage c JOIN path_dictionary p USING(path_id) JOIN coverage_expressions x USING(expression_id) ORDER BY p.path,c.line,x.expression,c.status",
                    "SELECT json_array(p.path,e.line,e.kind,s.id,d.id) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash ORDER BY p.path,e.line,e.kind,s.id,d.id",
                ] { assert_eq!(query_rows(&reopened,sql),query_rows(&cold,sql),"{phase}: reopened cold/delta identity"); }
                    assert_eq!(
                        all_local_facts(&reopened),
                        all_local_facts(&cold),
                        "{phase}: complete persisted facts"
                    );
                    commitments::verify(&reopened).unwrap();
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
            check("missing provider", "unresolved", false, false, "readParcel");
            workspace.write(rewrite(provider), &rewrite(provider_source));
            workspace.delta(&rewrite(provider));
            check(
                "provider-only addition",
                "resolved",
                true,
                true,
                "readParcel",
            );
            for (label, source, implicit, aliases, relative, target) in [
                ("export renamed", "export function otherParcel() { return 1; }\n", "unresolved", false, false, "readParcel"),
                ("unknown export", "export {readParcel} from './missing';\n", "unresolved", false, false, "readParcel"),
                ("local exported alias", "function actualParcel() { return 1; }\nexport {actualParcel as readParcel};\n", "resolved", true, true, "actualParcel"),
                ("provider-only alias retarget", "function alternateParcel() { return 2; }\nexport {alternateParcel as readParcel};\n", "resolved", true, true, "alternateParcel"),
                ("restore direct export", provider_source, "resolved", true, true, "readParcel"),
            ] {
                workspace.write(rewrite(provider), &rewrite(source));
                workspace.delta(&rewrite(provider));
                check(label,implicit,aliases,relative,target);
            }
            workspace.write(rewrite("providers/duplicate.ts"), &rewrite(provider_source));
            workspace.delta(&rewrite("providers/duplicate.ts"));
            check(
                "distinct complete duplicate",
                "ambiguous",
                true,
                true,
                "readParcel",
            );
            workspace.write(
                rewrite("providers/duplicate.ts"),
                &rewrite("export {readParcel} from './missing';\n"),
            );
            workspace.delta(&rewrite("providers/duplicate.ts"));
            check(
                "known plus unknown competitor",
                "nonexact",
                true,
                true,
                "readParcel",
            );
            fs::remove_file(workspace.join(rewrite("providers/duplicate.ts"))).unwrap();
            workspace.delta(&rewrite("providers/duplicate.ts"));
            check("duplicate removed", "resolved", true, true, "readParcel");
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
                workspace.delta(configuration);
                check(label, implicit, aliases, true, "readParcel");
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
            workspace.delta(manifest);
            check("descriptor absent", "unresolved", false, true, "readParcel");
            workspace.write(manifest, &policy_source);
            workspace.delta(manifest);
            check("descriptor restored", "resolved", true, true, "readParcel");
            workspace.write("package.json", r#"{"dependencies":{}}"#);
            workspace.delta("package.json");
            check("package removed", "unresolved", false, true, "readParcel");
            workspace.write("package.json", &rewrite(package));
            workspace.delta("package.json");
            check("package restored", "resolved", true, true, "readParcel");
            fs::remove_file(workspace.join(configuration)).unwrap();
            workspace.delta(configuration);
            check("config removed", "unresolved", false, true, "readParcel");
            workspace.write(configuration, &rewrite(configured));
            workspace.delta(configuration);
            check("config re-added", "resolved", true, true, "readParcel");
            fs::remove_file(workspace.join(rewrite(provider))).unwrap();
            workspace.delta(&rewrite(provider));
            check("provider removed", "unresolved", false, false, "readParcel");
            workspace.write(rewrite(provider), &rewrite(provider_source));
            workspace.delta(&rewrite(provider));
            check("provider restored", "resolved", true, true, "readParcel");
            fs::remove_file(workspace.join(manifest)).unwrap();
            workspace.delta(manifest);
            check("rule removed", "unresolved", false, true, "readParcel");
            workspace.write(manifest, &policy_source);
            workspace.delta(manifest);
            check("rule restored", "resolved", true, true, "readParcel");
            let sql = "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id";
            let before = query_rows(&workspace.open(), sql);
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
                    writer::build(&workspace.0, &workspace.db(), None).is_err(),
                    "renamed={renamed}: strict descriptor boundary"
                );
                commitments::verify(&workspace.open()).unwrap();
                assert_eq!(
                    query_rows(&workspace.open(), sql),
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
}
