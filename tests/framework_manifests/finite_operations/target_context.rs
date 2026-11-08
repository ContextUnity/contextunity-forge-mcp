use super::*;

#[test]
fn target_context_resolves_only_routes_in_declared_workspace_scope() {
    // Real test-client shape: identical paths in independent applications are
    // ambiguous until explicit owner policy asserts the intended application.
    let scoped = FrameworkHarness::persisted(&[]);
    scoped.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nexcluded_directory_names: [service]\nlinked_workspaces:\n  - name: service\n    path: '{}'\n    roots: [.]\n",
            scoped.path("service").display()
        ),
    );
    for prefix in ["", "service/"] {
        scoped.write(
            format!("{prefix}pyproject.toml"),
            "[project]\nname='route-proof'\ndependencies=['fastapi']\n",
        );
    }
    let route_source = "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/test')\ndef handler(): return 1\n";
    scoped.write("app.py", route_source);
    scoped.write("service/app.py", route_source);
    let client_source = "def request(client): return client.get('/test')\ndef missing(client): return client.get('/missing')\ndef wrong_method(client): return client.post('/test')\n";
    scoped.write("tests/test_api.py", client_source);
    scoped.write("outside.py", client_source);
    scoped.write(
        "ordinary.py",
        "def helper(): return 1\ndef ordinary(): return helper()\n",
    );
    scoped.build();
    let check_scope = |status: &str, target_path: Option<&str>| {
        let conn = scoped.open();
        commitments::verify(&conn).unwrap();
        assert!(error_messages(&conn).is_empty());
        assert_eq!(
            coverage_statuses_for_path(&conn, "tests/test_api.py", "GET /test"),
            [status]
        );
        assert_eq!(
            coverage_statuses_for_path(&conn, "outside.py", "GET /test"),
            ["ambiguous"],
            "out-of-context requests retain original ambiguity"
        );
        for expression in ["GET /missing", "POST /test"] {
            assert_eq!(
                coverage_statuses_for_path(&conn, "tests/test_api.py", expression),
                ["unresolved"]
            );
        }
        let targets = edge_dst_ids(&conn, "tests/test_api.py", "request", "calls_endpoint");
        let expected: Vec<String> = target_path
            .map(|path| node_id_by_kind(&conn, path, "route", "GET /test"))
            .into_iter()
            .collect();
        assert_eq!(
            targets, expected,
            "same context selects the actual canonical route"
        );
        assert_eq!(call_target_names(&conn, None, "ordinary"), ["helper"]);
        drop(conn);
        operations::check_cold(delta_cold_observations(&scoped), "filesystem cold");
    };
    check_scope("ambiguous", None);
    let web: Value = toml::from_str::<toml::Value>(include_str!(
        "../../../crates/forge-index/src/languages/manifests/web.toml"
    ))
    .unwrap()
    .try_into()
    .unwrap();
    let policy = |owners: &[&str]| {
        let mut web = web.clone();
        for rule in web["syntax_rules"].as_array_mut().unwrap() {
            if rule["id"] == "http-client-python" {
                rule["operation"]["target_context"] =
                    json!({"request_owners":["tests/test_api.py"],"route_owners":owners});
            }
        }
        toml::to_string(&web).unwrap()
    };
    let manifest = ".forge/frameworks/web.toml";
    for (owners, status, target) in [
        (vec!["app.py"], "resolved", Some("app.py")),
        (
            vec!["[service]/app.py"],
            "resolved",
            Some("[service]/app.py"),
        ),
        (vec!["missing.py"], "unresolved", None),
        (vec!["app.py", "[service]/app.py"], "ambiguous", None),
        (vec!["app.py"], "resolved", Some("app.py")),
    ] {
        scoped.write(manifest, &policy(&owners));
        scoped.delta(&[manifest]);
        check_scope(status, target);
    }
    // Provider removal/addition leaves the client unchanged and cannot fall back
    // to the equally shaped route in another workspace.
    scoped.remove("app.py");
    scoped.delta(&["app.py"]);
    // The outside request now has only the linked provider; preserve that old policy.
    let conn = scoped.open();
    assert_eq!(
        coverage_statuses_for_path(&conn, "tests/test_api.py", "GET /test"),
        ["unresolved"]
    );
    assert_eq!(
        query_count(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='tests/test_api.py' AND e.kind='calls_endpoint'",
            [],
        ),
        0
    );
    drop(conn);
    operations::check_cold(delta_cold_observations(&scoped), "filesystem cold");
    scoped.write("app.py", route_source);
    scoped.delta(&["app.py"]);
    check_scope("resolved", Some("app.py"));
    scoped.write(
        "tests/test_api.py",
        &format!("{client_source}\n# cached application provider\n"),
    );
    scoped.delta(&["tests/test_api.py"]);
    check_scope("resolved", Some("app.py"));
    scoped.remove(manifest);
    scoped.delta(&[manifest]);
    check_scope("ambiguous", None);
    scoped.write(manifest, &policy(&["app.py"]));
    scoped.delta(&[manifest]);
    check_scope("resolved", Some("app.py"));
}
