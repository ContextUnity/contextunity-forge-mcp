use super::*;
#[cfg(all(feature = "lang-typescript", feature = "lang-html"))]
#[test]
fn gridviewspec_pack_bindings_follow_unique_targets() {
    use contextunity_forge_mcp::core::commitments;

    let workspace = fixture_workspace(&[]);
    let families = [
        ("table_v2", "GridViewTable", "datasource"),
        ("toolbar", "GridViewToolbar", "counters"),
        ("filters_v2", "GridViewFilters", "drawer_sections"),
        ("content", "GridViewCards", "cards"),
        ("content", "GridViewCharts", "charts"),
        ("content", "GridViewKpi", "items"),
        ("content", "GridViewTabs", "tabs"),
        ("actions", "GridViewActions", "items"),
    ];
    let mut receivers = String::new();
    for (index, (module, base, member)) in families.iter().enumerate() {
        receivers.push_str(&format!(
            "from grid_view_spec.types.{module} import {base}\nclass Family{index}({base}):\n    def known(self): return self.{member}\n    def unknown(self): return self.unconfigured\n"
        ));
    }
    workspace.write("families.py", &receivers);
    workspace.write("backend.py", "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/api/records/')\ndef records(): return []\n@app.post('/api/records/')\ndef update(): return []\n@app.get('/api/duplicate/')\ndef first(): return []\n@app.get('/api/duplicate/')\ndef second(): return []\n");
    workspace.write(
        "ordinary.py",
        "from backend import records\ndef ordinary(): return records()\n",
    );
    let provider = "export function draw() { return 1; }\nexport function openCart() { return 2; }\nexport function install() {\n    var gv = window.GridView;\n    gv.registerRenderer('stock_cart', draw);\n    gv.registerAction('open_stock_cart', openCart);\n    gv.registerRenderer('duplicate', draw);\n    gv.registerRenderer('duplicate', openCart);\n}\nexport function shadow(window) { window.GridView.registerRenderer('shadow', draw); }\nexport function rebound() { var gv = window.GridView; gv = other; gv.registerRenderer('rebound', draw); }\n";
    let consumer = "from grid_view_spec.types.table_v2 import GridViewTable, GridViewDataSource, GridViewColumn\ndef get_table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/'))\ndef post_table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/', method='post'))\ndef renderer(): return GridViewColumn(renderer='stock_cart')\ndef action(): return GridViewColumn(extra={'action':'open_stock_cart'})\ndef missing(): return GridViewColumn(renderer='missing')\ndef duplicate(): return GridViewColumn(renderer='duplicate')\ndef dynamic(): return GridViewColumn(renderer=selected)\ndef shadowed(GridViewColumn): return GridViewColumn(renderer='stock_cart')\ndef shadow(): return GridViewColumn(renderer='shadow')\ndef rebound(): return GridViewColumn(renderer='rebound')\ndef dynamic_url(): return GridViewTable(datasource=GridViewDataSource(endpoint=selected))\ndef dynamic_method(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/', method=selected))\ndef detached(): return GridViewDataSource(endpoint='/api/records/')\ndef missing_route(): return GridViewTable(datasource=GridViewDataSource(endpoint='/absent/'))\ndef duplicate_route(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/duplicate/'))\ndef shadow_table(GridViewTable): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/'))\n";
    workspace.write("provider.js", provider);
    workspace.write("spec.py", consumer);
    workspace.write("request.html", "<button hx-get='/api/records/' hx-target='#records' hx-swap='outerHTML'></button>\n<button hx-get='/api/{{ id }}/'></button>\n");
    workspace.build();

    let observe = |active: bool, registry: &str, family_active: bool| {
        let conn = workspace.open();
        commitments::verify(&conn).unwrap();
        assert!(query_strings(&conn, "SELECT message FROM errors", []).is_empty());
        for (index, (module, base, member)) in families.iter().enumerate() {
            let (status, evidence) =
                persisted_coverage(&conn, "families.py", (index * 4 + 3) as i64, &format!("self.{member}"));
            assert_eq!(
                status,
                if family_active { "external" } else { "unresolved" },
                "{base}: {evidence}"
            );
            if family_active {
                assert!(
                    evidence.contains(&format!("grid_view_spec.types.{module}.{base}")),
                    "{evidence}"
                );
            }
            assert_eq!(
                persisted_coverage(&conn, "families.py", (index * 4 + 4) as i64, "self.unconfigured").0,
                "unresolved"
            );
        }
        let target = |path: &str, name: &str| -> String {
            query_scalar(
                &conn,
                "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2",
                (path, name),
            )
        };
        assert_eq!(
            query_strings(
                &conn,
                "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='ordinary.py' AND s.name='ordinary' AND e.kind='calls'",
                []
            ),
            [target("backend.py", "records")]
        );
        assert_eq!(persisted_coverage(&conn, "ordinary.py", 1, "records").0, "resolved");
        let mut failures = Vec::new();
        for (source, kind, path, name, expected) in [
            ("get_table", "calls_endpoint", "backend.py", "GET /api/records/", active),
            (
                "post_table",
                "calls_endpoint",
                "backend.py",
                "POST /api/records/",
                active,
            ),
            (
                "renderer",
                "references",
                "provider.js",
                "draw",
                active && registry == "resolved",
            ),
            (
                "action",
                "references",
                "provider.js",
                "openCart",
                active && registry == "resolved",
            ),
        ] {
            let actual = query_strings(
                &conn,
                "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='spec.py' AND s.name=?1 AND e.kind=?2 ORDER BY d.id",
                (source, kind),
            );
            let expected = if expected { vec![target(path, name)] } else { vec![] };
            if actual != expected {
                failures.push(format!(
                    "{source} --{kind}--> {path}:{name}: expected {expected:?}, actual {actual:?}"
                ));
            }
        }
        for (source, kind) in [
            ("missing", "references"),
            ("duplicate", "references"),
            ("dynamic", "references"),
            ("shadowed", "references"),
            ("shadow", "references"),
            ("rebound", "references"),
            ("dynamic_url", "calls_endpoint"),
            ("dynamic_method", "calls_endpoint"),
            ("detached", "calls_endpoint"),
            ("missing_route", "calls_endpoint"),
            ("duplicate_route", "calls_endpoint"),
            ("shadow_table", "calls_endpoint"),
        ] {
            let count: usize = query_scalar(
                &conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='spec.py' AND s.name=?1 AND e.kind=?2",
                (source, kind),
            );
            assert_eq!(count, 0, "nonexact {source} --{kind}");
        }
        assert_eq!(
            query_strings(
                &conn,
                "SELECT s.name||'->'||d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='backend.py' AND e.kind='handles' AND s.name IN ('GET /api/records/','POST /api/records/') ORDER BY s.name",
                []
            ),
            [
                format!("GET /api/records/->{}", target("backend.py", "records")),
                format!("POST /api/records/->{}", target("backend.py", "update"))
            ]
        );
        assert_eq!(
            query_strings(
                &conn,
                "SELECT d.id FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='request.html' AND e.kind='calls_endpoint'",
                []
            ),
            [target("backend.py", "GET /api/records/")]
        );
        if active {
            for (line, expression, status) in [
                (6, "%missing%", "unresolved"),
                (7, "%duplicate%", "ambiguous"),
                (15, "%/absent/%", "unresolved"),
                (16, "%/api/duplicate/%", "ambiguous"),
                (4, "%stock_cart%", registry),
                (5, "%open_stock_cart%", registry),
            ] {
                let count: usize = query_scalar(
                    &conn,
                    "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='spec.py' AND c.line=?1 AND x.expression LIKE ?2 AND c.status=?3",
                    (line, expression, status),
                );
                if count == 0 {
                    failures.push(format!("spec.py:{line} {expression}: missing {status} coverage"));
                }
            }
        }
        assert!(failures.is_empty(), "GridViewSpec bindings:\n{}", failures.join("\n"));
        drop(conn);
    };
    let persist_parity = || {
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
    observe(true, "resolved", true);

    // A workspace extension adds a project API without replacing the verified catalogue.
    let bundled: toml::Value = toml::from_str(include_str!(
        "../../../../crates/forge-index/src/languages/manifests/gridviewspec.toml"
    ))
    .unwrap();
    assert_eq!(bundled["class_hierarchy_receivers"].as_array().unwrap().len(), 28);
    workspace.write(
        "extension.py",
        "from fixture_grid import Widget\nclass Extended(Widget):\n    def known(self): return self.expose\n",
    );
    workspace.delta(&["extension.py"]);
    assert_eq!(
        persisted_coverage(&workspace.open(), "extension.py", 3, "self.expose").0,
        "unresolved"
    );
    workspace.write(".forge/frameworks/gridviewspec.toml", "[[class_hierarchy_receivers]]\nbases=['fixture_grid.Widget']\nprovider='fixture_grid.Widget'\nmembers=['expose']\n");
    workspace.delta(&[".forge/frameworks/gridviewspec.toml"]);
    assert_eq!(
        persisted_coverage(&workspace.open(), "extension.py", 3, "self.expose").0,
        "external"
    );
    observe(true, "resolved", true);
    remove_fixture(&workspace, ".forge/frameworks/gridviewspec.toml");
    workspace.delta(&[".forge/frameworks/gridviewspec.toml"]);
    assert_eq!(
        persisted_coverage(&workspace.open(), "extension.py", 3, "self.expose").0,
        "unresolved"
    );

    workspace.write(
        "provider.js",
        &provider
            .replace("gv.registerRenderer('stock_cart', draw);", "")
            .replace("gv.registerAction('open_stock_cart', openCart);", ""),
    );
    workspace.delta(&["provider.js"]);
    observe(true, "unresolved", true);
    workspace.write(
        "provider.js",
        &provider
            .replace(
                "gv.registerRenderer('stock_cart', draw);",
                "gv.registerRenderer('stock_cart', draw); gv.registerRenderer('stock_cart', openCart);",
            )
            .replace(
                "gv.registerAction('open_stock_cart', openCart);",
                "gv.registerAction('open_stock_cart', openCart); gv.registerAction('open_stock_cart', draw);",
            ),
    );
    workspace.delta(&["provider.js"]);
    observe(true, "ambiguous", true);
    workspace.write("provider.js", provider);
    workspace.delta(&["provider.js"]);
    observe(true, "resolved", true);

    let manifest = ".forge/frameworks/gridviewspec.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../../crates/forge-index/src/languages/manifests/gridviewspec.toml"
    ))
    .unwrap();
    pack.as_table_mut().unwrap().insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed.as_table_mut().unwrap().remove("semantic_rules");
    removed.as_table_mut().unwrap().remove("syntax_rules");
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(&[manifest]);
    observe(false, "unresolved", true);
    workspace.write(manifest, "replace=true\n");
    workspace.delta(&[manifest]);
    observe(false, "unresolved", false);
    let neighbor = fixture_workspace(&[]);
    neighbor.write("families.py", &receivers);
    neighbor.build();
    assert_eq!(
        persisted_coverage(&neighbor.open(), "families.py", 3, "self.datasource").0,
        "external"
    );
    remove_fixture(&workspace, manifest);
    workspace.delta(&[manifest]);
    observe(true, "resolved", true);

    let rewrite = |text: &str| {
        text.replace("grid_view_spec.types.table_v2", "atelier.tables")
            .replace("GridViewTable", "Panel")
            .replace("GridViewDataSource", "Feed")
            .replace("GridViewColumn", "Cell")
            .replace("datasource", "sourcefeed")
            .replace("endpoint", "address")
            .replace("method", "mode")
            .replace("renderer", "painter")
            .replace("extra", "options")
            .replace("action", "command")
            .replace("GridView", "Surface")
            .replace("registerRenderer", "bindPainter")
            .replace("registerAction", "bindCommand")
    };
    for section in ["semantic_rules", "syntax_rules"] {
        for rule in pack[section].as_array_mut().expect("pack provides binding rules") {
            // Rewrite configured identities only, preserving typed schema keys.
            let mut value = serde_json::to_value(&*rule).unwrap();
            fn rewrite_values(value: &mut serde_json::Value, rewrite: &impl Fn(&str) -> String) {
                match value {
                    serde_json::Value::String(text) => *text = rewrite(text),
                    serde_json::Value::Array(items) => {
                        for item in items {
                            rewrite_values(item, rewrite);
                        }
                    }
                    serde_json::Value::Object(fields) => {
                        for item in fields.values_mut() {
                            rewrite_values(item, rewrite);
                        }
                    }
                    _ => {}
                }
            }
            rewrite_values(&mut value, &|text| {
                if [
                    "endpoint_projection",
                    "named_binding",
                    "call_definition",
                    "call_reference",
                    "keyword_member",
                    "global_member",
                    "imported",
                    "argument",
                    "keyword",
                    "callable",
                    "python",
                    "javascript",
                ]
                .contains(&text)
                {
                    text.into()
                } else {
                    rewrite(text)
                }
            });
            *rule = toml::Value::try_from(&value).unwrap();
        }
    }
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(&[manifest]);
    observe(false, "unresolved", true);
    let rewritten_consumer = rewrite(consumer)
        .replace("def painter(", "def renderer(")
        .replace("def command(", "def action(")
        .replace("def dynamic_mode(", "def dynamic_method(");
    workspace.write("spec.py", &rewritten_consumer);
    workspace.delta(&["spec.py"]);
    workspace.write("provider.js", &rewrite(provider));
    workspace.delta(&["provider.js"]);
    observe(true, "resolved", true);
    remove_fixture(&workspace, manifest);
    workspace.delta(&[manifest]);
    observe(false, "unresolved", true);
    workspace.write("spec.py", consumer);
    workspace.delta(&["spec.py"]);
    workspace.write("provider.js", provider);
    workspace.delta(&["provider.js"]);
    observe(true, "resolved", true);
    persist_parity();

}

#[cfg(all(feature = "lang-typescript", feature = "lang-html"))]
#[test]
fn gridviewspec_runtime_provider_scopes_canonical() {
    assert_gridviewspec_runtime_provider_scopes(false);
}

#[cfg(all(feature = "lang-typescript", feature = "lang-html"))]
#[test]
fn gridviewspec_runtime_provider_scopes_renamed() {
    assert_gridviewspec_runtime_provider_scopes(true);
}

#[cfg(all(feature = "lang-typescript", feature = "lang-html"))]
fn assert_gridviewspec_runtime_provider_scopes(renamed: bool) {
    use contextunity_forge_mcp::core::commitments;

    // Observed built-in runtime registrations live in a separately indexed
    // frontend project. Their installation-specific workspace identity is local data.
    let bundled: toml::Value = toml::from_str(include_str!(
        "../../../../crates/forge-index/src/languages/manifests/gridviewspec.toml"
    ))
    .unwrap();
    let mut runtime_policy = bundled;
    for rule in runtime_policy["syntax_rules"].as_array_mut().unwrap() {
        if rule["id"].as_str() == Some("renderer-reference") {
            rule["operation"]["binding"].as_table_mut().unwrap().insert(
                "provider_scopes".into(),
                vec![
                    toml::Value::try_from(
                        serde_json::json!({"workspace":"runtime","project":"frontend","package":"runtime-provider"}),
                    )
                    .unwrap(),
                ]
                .into(),
            );
        }
    }
    let runtime_policy = toml::to_string(&runtime_policy).unwrap();
    let scoped = fixture_workspace(&[]);
    let runtime = fixture_workspace(&[]);
    let rewrite = |text: &str| {
        if renamed {
            text.replace("grid_view_spec.types.table_v2", "atelier.tables")
                .replace("GridViewColumn", "Cell")
                .replace("registerRenderer", "bindPainter")
                .replace("gridviewspec_renderer", "atelier_painter")
        } else {
            text.to_owned()
        }
    };
    scoped.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: runtime\n    path: '{}'\n    roots: [.]\n",
            runtime.root().display()
        ),
    );
    scoped.write(
        "pyproject.toml",
        "[project]\nname='consumer'\ndependencies=['grid-view-spec']\n",
    );
    runtime.write("pyproject.toml", "[project]\nname='grid-view-spec'\n");
    runtime.write("frontend/package.json", "{\"name\":\"runtime-provider\"}");
    let provider_path = "frontend/src/runtime/renderers/builtins.ts";
    let runtime_source = rewrite(
        "import {registerRenderer} from '../../grid-view/registry-api';\nfunction moneyRenderer() { return 'money'; }\nfunction linkRenderer() { return 'link'; }\nexport function initBuiltinRenderers() { registerRenderer('money', moneyRenderer); }\n",
    );
    runtime.write(provider_path, &runtime_source);
    let consumer = rewrite(
        "from grid_view_spec.types.table_v2 import GridViewColumn\ndef render(): return GridViewColumn(renderer='money')\ndef missing(): return GridViewColumn(renderer='r')\ndef dynamic(): return GridViewColumn(renderer=selected)\ndef shadow(GridViewColumn): return GridViewColumn(renderer='money')\n",
    );
    scoped.write("consumer.py", &consumer);
    runtime.write("src/consumer.py", &consumer);
    let policy = rewrite(&runtime_policy);
    let local_manifest = ".forge/frameworks/gridviewspec.toml";
    scoped.write(local_manifest, &policy);
    runtime.write(local_manifest, &policy);
    scoped.build();
    let check = |expected: Option<&str>, status: &str| {
        let conn = scoped.open();
        commitments::verify(&conn).unwrap();
        assert!(query_strings(&conn, "SELECT message FROM errors", []).is_empty());
        let targets: Vec<String> = query_strings(
            &conn,
            "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name='render' AND e.kind='references' ORDER BY d.id",
            [],
        );
        let wanted: Vec<String> = expected
            .map(|name| {
                query_scalar(
                    &conn,
                    "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='[runtime]/frontend/src/runtime/renderers/builtins.ts' AND n.name=?1",
                    [name],
                )
            })
            .into_iter()
            .collect();
        assert_eq!(
            targets, wanted,
            "configured runtime exact canonical callback, renamed={renamed}"
        );
        assert_eq!(persisted_coverage(&conn, "consumer.py", 2, "money").0, status);
        for function in ["missing", "dynamic", "shadow"] {
            let count: i64 = query_scalar(
                &conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name=?1 AND e.kind='references'",
                [function],
            );
            assert_eq!(count, 0, "no runtime guess for {function}");
        }
        drop(conn);
        let parity = crate::languages::harness::cold_relations(&scoped);
        assert_eq!(parity.incremental_verification, Ok(()));
        assert_eq!(parity.cold_verification, Ok(()));
        assert_eq!(parity.incremental, parity.cold);
    };
    check(Some("moneyRenderer"), "resolved");
    assert_eq!(
        persisted_coverage(&scoped.open(), "[runtime]/src/consumer.py", 2, "money").0,
        "resolved",
        "same-workspace Python/frontend boundary also requires explicit import"
    );
    // Unchanged consumers must replay a callback-only provider mutation.
    runtime.write(
        provider_path,
        &runtime_source.replace("('money', moneyRenderer)", "('money', linkRenderer)"),
    );
    scoped.delta(&["[runtime]/frontend/src/runtime/renderers/builtins.ts"]);
    check(Some("linkRenderer"), "resolved");
    runtime.write(
        provider_path,
        &format!(
            "{runtime_source}\nexport function duplicate() {{ {}('money', linkRenderer); }}\n",
            if renamed { "bindPainter" } else { "registerRenderer" }
        ),
    );
    scoped.delta(&["[runtime]/frontend/src/runtime/renderers/builtins.ts"]);
    check(None, "ambiguous");
    remove_fixture(&runtime, provider_path);
    scoped.delta(&["[runtime]/frontend/src/runtime/renderers/builtins.ts"]);
    check(None, "unresolved");
    runtime.write(provider_path, &runtime_source);
    scoped.delta(&["[runtime]/frontend/src/runtime/renderers/builtins.ts"]);
    check(Some("moneyRenderer"), "resolved");
    scoped.write(
        local_manifest,
        &policy.replace("package = \"runtime-provider\"", "package = \"unrelated-provider\""),
    );
    scoped.delta(&[local_manifest]);
    check(None, "unresolved");
    scoped.write(local_manifest, &policy);
    scoped.delta(&[local_manifest]);
    check(Some("moneyRenderer"), "resolved");
    scoped.write("consumer.py", &format!("{consumer}\n# cached provider\n"));
    scoped.delta(&["consumer.py"]);
    check(Some("moneyRenderer"), "resolved");
}
