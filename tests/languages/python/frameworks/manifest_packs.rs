use super::*;
#[test]
fn starlette_and_fastapi_pack_relations_are_exact() {
    use contextunity_forge_mcp::core::commitments;

    let workspace = fixture_workspace(&[
        (
            "pyproject.toml",
            "[project]\nname='python-framework-packs'\ndependencies=['fastapi','starlette','shorekit']\n",
        ),
        (
            ".forge/frameworks/starlette.toml",
            include_str!("../../../../crates/forge-index/src/languages/manifests/starlette.toml"),
        ),
        (
            "models.py",
            "class Record: pass\nclass Alternate: pass\nclass Fault: pass\nclass Duplicate: pass\nclass Duplicate: pass\n",
        ),
        (
            "providers.py",
            "def provide(): return 1\ndef repeated(): return 1\ndef repeated(): return 2\n",
        ),
        (
            "ordinary.py",
            "from providers import provide\ndef ordinary(): return provide()\n",
        ),
        (
            "routes.py",
            "from fastapi import FastAPI, APIRouter\nfrom starlette.routing import Route, Mount, WebSocketRoute\napp = FastAPI()\nparent = APIRouter(prefix='/api')\nleaf = APIRouter(prefix='/v1')\napp.include_router(parent, prefix='/root')\nparent.include_router(leaf, prefix='/nested')\n@leaf.get('/records')\ndef nested(): pass\n@leaf.get(dynamic_path)\ndef dynamic_route(): pass\ndef http(): pass\ndef socket(): pass\nroutes = [Route('/plain', http), Mount('/mounted'), WebSocketRoute('/socket', socket)]\n",
        ),
        (
            "responses.py",
            "from starlette.responses import JSONResponse, HTMLResponse\nclass Json(JSONResponse):\n    def known(self): return self.render({})\nclass Html(HTMLResponse):\n    def known(self): return self.render('ok')\n    def unknown(self): return self.unconfigured()\n",
        ),
    ]);
    let consumer = "from fastapi import FastAPI, Depends\nfrom models import Record, Alternate, Fault, Duplicate\nfrom providers import provide, repeated\napp = FastAPI()\n@app.get('/records', response_model=Record)\ndef endpoint(value=Depends(provide)): return value\n@app.exception_handler(Fault)\ndef rescue(): return None\ndef registered(): return None\napp.add_exception_handler(Fault, registered)\n@app.get('/missing', response_model=MissingModel)\ndef missing_model(): return None\n@app.get('/duplicate', response_model=Duplicate)\ndef duplicate_model(): return None\n@app.get('/dynamic', response_model=choose_model())\ndef dynamic_model(): return None\ndef missing_dependency(value=Depends(absent)): return value\ndef duplicate_dependency(value=Depends(repeated)): return value\n@app.exception_handler(MissingFault)\ndef missing_fault(): return None\n@app.exception_handler(Duplicate)\ndef duplicate_fault(): return None\napp.add_exception_handler(Fault, absent_handler)\ndef shadow(app, Depends):\n    @app.get('/shadow', response_model=Record)\n    def blocked(value=Depends(provide)): return value\n    return blocked\n";
    workspace.write("consumer.py", consumer);
    workspace.build();

    let observe = |active: bool| {
        let conn = workspace.open();
        commitments::verify(&conn).unwrap();
        assert!(query_strings(&conn, "SELECT message FROM errors", []).is_empty());
        let handles = query_strings(
            &conn,
            "SELECT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='routes.py' AND e.kind='handles' ORDER BY s.name,d.name",
            [],
        );
        assert_eq!(
            handles,
            [
                "ANY /plain->http",
                "GET /root/api/nested/v1/records->nested",
                "WEBSOCKET /socket->socket",
            ]
        );
        assert_eq!(
            query_strings(
                &conn,
                "SELECT name FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='routes.py' AND n.kind='route' AND n.name LIKE 'MOUNT %'",
                []
            ),
            ["MOUNT /mounted"]
        );
        for (line, expression, provider) in [
            (3, "self.render", "starlette.responses.JSONResponse"),
            (5, "self.render", "starlette.responses.HTMLResponse"),
        ] {
            let (status, evidence) = persisted_coverage(&conn, "responses.py", line, expression);
            assert_eq!(status, "external", "{evidence}");
            assert!(evidence.contains(provider), "{evidence}");
        }
        assert_eq!(
            persisted_coverage(&conn, "responses.py", 6, "self.unconfigured").0,
            "unresolved"
        );
        assert_eq!(
            query_strings(
                &conn,
                "SELECT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='ordinary.py' AND e.kind='calls'",
                []
            ),
            ["ordinary->provide"]
        );
        assert_eq!(persisted_coverage(&conn, "ordinary.py", 1, "provide").0, "resolved");

        let mut failures = Vec::new();
        for (source, kind, target_path, target) in [
            ("endpoint", "injects_dependency", "providers.py", "provide"),
            ("endpoint", "uses_model", "models.py", "Record"),
            ("rescue", "references", "models.py", "Fault"),
            ("registered", "references", "models.py", "Fault"),
        ] {
            let count: usize = query_scalar(
                &conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path='consumer.py' AND s.name=?1 AND e.kind=?2 AND dp.path=?3 AND d.name=?4",
                (source, kind, target_path, target),
            );
            if count != usize::from(active) {
                failures.push(format!(
                    "{source} --{kind}--> {target_path}:{target}: expected {}, observed {count}",
                    usize::from(active)
                ));
            }
        }
        for (source, kind) in [
            ("missing_model", "uses_model"),
            ("duplicate_model", "uses_model"),
            ("dynamic_model", "uses_model"),
            ("blocked", "uses_model"),
            ("blocked", "injects_dependency"),
            ("missing_dependency", "injects_dependency"),
            ("duplicate_dependency", "injects_dependency"),
            ("missing_fault", "references"),
            ("duplicate_fault", "references"),
        ] {
            let count: usize = query_scalar(
                &conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name=?1 AND e.kind=?2",
                (source, kind),
            );
            assert_eq!(count, 0, "nonexact {source} --{kind}");
        }
        if active {
            for (marker, expression, status) in [
                ("@app.get('/missing'", "%MissingModel%", "unresolved"),
                ("@app.get('/duplicate'", "%Duplicate%", "ambiguous"),
                ("missing_dependency(value", "%absent%", "unresolved"),
                ("duplicate_dependency(value", "%repeated%", "ambiguous"),
                ("@app.exception_handler(MissingFault)", "%MissingFault%", "unresolved"),
                ("@app.exception_handler(Duplicate)", "%Duplicate%", "ambiguous"),
            ] {
                let line = consumer.lines().position(|line| line.contains(marker)).unwrap() + 1;
                let count: usize = query_scalar(
                    &conn,
                    "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='consumer.py' AND c.line=?1 AND x.expression LIKE ?2 AND c.status=?3",
                    (line, expression, status),
                );
                if count == 0 {
                    failures.push(format!("consumer.py:{line} {expression}: missing {status} coverage"));
                }
            }
        }
        assert!(failures.is_empty(), "pack semantic contract:\n{}", failures.join("\n"));
        drop(conn);
        let parity = crate::languages::harness::cold_relations(&workspace);
        assert_eq!(parity.incremental_verification, Ok(()));
        assert_eq!(parity.cold_verification, Ok(()));
        assert_eq!(parity.incremental, parity.cold);
        let cold = parity.cold_connection;
        let incremental = workspace.open();
        for sql in [
            "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
            "SELECT json_array(path,hex(facts_blob)) FROM local_facts ORDER BY path",
            "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
        ] {
            assert_eq!(
                query_strings(&incremental, sql, []),
                query_strings(&cold, sql, []),
                "{sql}"
            );
        }
    };
    observe(true);

    let manifest = ".forge/frameworks/fastapi.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../../crates/forge-index/src/languages/manifests/fastapi.toml"
    ))
    .unwrap();
    pack.as_table_mut().unwrap().insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed.as_table_mut().unwrap().remove("semantic_rules");
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(&[manifest]);
    observe(false);

    let rewrite = |source: &str| {
        source
            .replace("fastapi", "shorekit")
            .replace("FastAPI", "Gateway")
            .replace("Depends", "Acquire")
            .replace("response_model", "result_type")
            .replace("add_exception_handler", "install_rescue")
            .replace("exception_handler", "rescue_handler")
    };
    for rule in pack["semantic_rules"].as_array_mut().unwrap() {
        *rule = toml::from_str::<toml::Value>(&rewrite(&toml::to_string(rule).unwrap())).unwrap();
    }
    pack["di_calls"][0]["callee"] = "Acquire".into();
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(&[manifest]);
    observe(false);
    workspace.write("consumer.py", &rewrite(consumer));
    workspace.delta(&["consumer.py"]);
    observe(true);

    remove_fixture(&workspace, manifest);
    workspace.delta(&[manifest]);
    observe(false);
    workspace.write("consumer.py", consumer);
    workspace.delta(&["consumer.py"]);
    observe(true);
}
