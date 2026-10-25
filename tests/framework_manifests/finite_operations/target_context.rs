use super::*;
use serde_json::json;

pub(super) fn run() {
    // Real test-client shape: identical paths in independent applications are
    // ambiguous until explicit owner policy asserts the intended application.
    let scoped = Workspace::new();
    let linked = Workspace::new();
    scoped.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: service\n    path: '{}'\n    roots: [.]\n",
            linked.0.display()
        ),
    );
    for workspace in [&scoped, &linked] {
        workspace.write(
            "pyproject.toml",
            "[project]\nname='route-proof'\ndependencies=['fastapi']\n",
        );
    }
    let route_source = "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/test')\ndef handler(): return 1\n";
    scoped.write("app.py", route_source);
    linked.write("app.py", route_source);
    let client_source = "def request(client): return client.get('/test')\ndef missing(client): return client.get('/missing')\ndef wrong_method(client): return client.post('/test')\n";
    scoped.write("tests/test_api.py", client_source);
    scoped.write("outside.py", client_source);
    scoped.write(
        "ordinary.py",
        "def helper(): return 1\ndef ordinary(): return helper()\n",
    );
    writer::build(&scoped.0, &scoped.db(), None).unwrap();
    let check_scope = |status: &str, target_path: Option<&str>| {
        let conn = scoped.open();
        commitments::verify(&conn).unwrap();
        assert!(query_rows(&conn, "SELECT message FROM errors").is_empty());
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
        assert_eq!(query_rows(&conn, "SELECT d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='ordinary' AND e.kind='calls'"), ["helper"]);
        drop(conn);
        assert_delta_cold_parity(&scoped);
    };
    check_scope("ambiguous", None);
    let web: Value = toml::from_str::<toml::Value>(include_str!(
        "../../../src/engine/languages/manifests/web.toml"
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
        scoped.delta(manifest);
        check_scope(status, target);
    }
    // Provider removal/addition leaves the client unchanged and cannot fall back
    // to the equally shaped route in another workspace.
    fs::remove_file(scoped.0.join("app.py")).unwrap();
    scoped.delta("app.py");
    // The outside request now has only the linked provider; preserve that old policy.
    let conn = scoped.open();
    assert_eq!(
        coverage_statuses_for_path(&conn, "tests/test_api.py", "GET /test"),
        ["unresolved"]
    );
    assert_eq!(query_count(&conn, "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='tests/test_api.py' AND e.kind='calls_endpoint'", []), 0);
    drop(conn);
    assert_delta_cold_parity(&scoped);
    scoped.write("app.py", route_source);
    scoped.delta("app.py");
    check_scope("resolved", Some("app.py"));
    scoped.write(
        "tests/test_api.py",
        &format!("{client_source}\n# cached application provider\n"),
    );
    scoped.delta("tests/test_api.py");
    check_scope("resolved", Some("app.py"));
    fs::remove_file(scoped.0.join(manifest)).unwrap();
    scoped.delta(manifest);
    check_scope("ambiguous", None);
    scoped.write(manifest, &policy(&["app.py"]));
    scoped.delta(manifest);
    check_scope("resolved", Some("app.py"));
}
