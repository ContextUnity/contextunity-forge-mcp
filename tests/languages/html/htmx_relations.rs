use super::*;

#[test]
fn htmx_and_alpine_relations_follow_exact_targets_and_effective_rules() {
    let workspace = Workspace::new();
    let methods = ["get", "post", "put", "patch", "delete"];
    let mut backend = String::from(
        "from fastapi import FastAPI\nfrom django.urls import path\napp = FastAPI()\n",
    );
    let mut html = String::new();
    for method in methods {
        backend.push_str(&format!("@app.{method}('/api/{method}')\ndef api_{method}(): return 1\ndef django_{method}(request): return 1\n"));
        html.push_str(&format!("<button hx-{method}='/api/{method}' hx-target='#result' hx-swap='outerHTML'></button>\n<button data-hx-{method}='/django/{method}'></button>\n"));
    }
    backend.push_str("urlpatterns = [\n");
    for method in methods {
        backend.push_str(&format!("    path('django/{method}', django_{method}),\n"));
    }
    backend.push_str("]\n@app.get('/items/{id}')\ndef item(): return 1\n@app.get('/duplicate')\ndef first(): return 1\n@app.get('/duplicate')\ndef second(): return 2\ndef helper(): return 1\ndef ordinary(): return helper()\n");
    let rejected = [
        ("/missing", Some("unresolved")),
        ("/duplicate", Some("ambiguous")),
        ("https://example.invalid/api/get", None),
        ("//example.invalid/api/get", None),
        ("/items/${id}", None),
        ("/items/{{ id }}", None),
        ("", None),
        ("#result", None),
    ];
    for (url, _) in rejected {
        html.push_str(&format!("<button hx-get='{url}'></button>\n"));
    }
    html.push_str("<div hx-target='/api/get' hx-swap='/api/post'></div>\n");
    let stores = "Alpine.store('cart', {checkout() { return 1; }});\n";
    let consumers = "import { imported } from './ordinary.js';\nexport function run() { return $store.cart.checkout(); }\nexport function emit() { return $dispatch('cart-open'); }\nexport function missingStore() { return $store.absent.checkout(); }\nexport function missingEvent() { return $dispatch('absent'); }\nexport function shadowStore($store) { return $store.cart.checkout(); }\nexport function shadowEvent($dispatch) { return $dispatch('cart-open'); }\nexport function helper() { return 1; }\nexport function ordinary() { return helper() + imported(); }\n";
    let listener = "<button x-on:cart-open='ordinary()'></button>\n";
    let fixture = [
        ("backend.py", backend.as_str()),
        ("page.html", html.as_str()),
        ("stores.js", stores),
        ("consumer.js", consumers),
        ("listener.html", listener),
        ("ordinary.js", "export function imported() { return 1; }\n"),
    ];
    for (path, source) in fixture {
        workspace.write(path, source);
    }
    // Explicit frontend facts must work without package.json or pyproject.toml.
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let observe = |requests: bool, bindings: bool, renamed: bool, target_state: &str| {
        let conn = workspace.open();
        assert_valid(&conn);
        let mut failures = Vec::new();
        let rows: Vec<(usize, String)> = conn.prepare("SELECT e.line,d.id FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='page.html' AND e.kind='calls_endpoint' ORDER BY e.line,d.id").unwrap()
            .query_map([], |r| Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        let mut expected = Vec::new();
        for (index, method) in methods.iter().enumerate() {
            for (offset, handler) in [
                (0, format!("api_{method}")),
                (1, format!("django_{method}")),
            ] {
                let handler_id = node_id(&conn, "backend.py", &handler);
                let routes: Vec<String> = conn.prepare("SELECT s.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='handles' AND d.id=?1 ORDER BY s.id").unwrap()
                    .query_map([handler_id], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(
                    routes.len(),
                    1,
                    "actual canonical route handles {handler}: {routes:?}"
                );
                if requests {
                    expected.push((index * 2 + offset + 1, routes[0].clone()));
                }
            }
        }
        if rows != expected {
            failures.push(format!(
                "HTMX exact routes/fail-closed controls: actual={rows:?}, expected={expected:?}"
            ));
        }
        if requests {
            for (index, (_, status)) in rejected.iter().enumerate() {
                if let Some(status) = status {
                    let statuses: Vec<String> = conn.prepare("SELECT DISTINCT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path='page.html' AND c.line=?1 ORDER BY c.status").unwrap()
                        .query_map([11 + index], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                    assert_eq!(
                        statuses,
                        vec![status.to_string()],
                        "HTMX rejected target at {}",
                        11 + index
                    );
                }
            }
            let contexts: usize = conn.query_row("SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='page.html' AND n.kind='htmx_request' AND json_extract(n.details,'$.target')='#result' AND json_extract(n.details,'$.swap')='outerHTML'", [], |r|r.get(0)).unwrap();
            assert_eq!(contexts, 5, "target/swap remain context on actual requests");
        }
        let edge_count = |source: &str,
                          kind: &str,
                          target_path: &str,
                          target_name: &str|
         -> usize {
            conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=d.path_id JOIN path_dictionary sp ON sp.path_id=s.path_id WHERE sp.path='consumer.js' AND s.name=?1 AND e.kind=?2 AND p.path=?3 AND d.name LIKE ?4", params![source,kind,target_path,target_name], |r|r.get(0)).unwrap()
        };
        let unique = bindings && target_state == "unique";
        for (source, kind, path, target, active) in [
            ("run", "calls", "stores.js", "checkout", unique),
            (
                "emit",
                "references",
                "listener.html",
                "alpine_event@%",
                unique,
            ),
            ("missingStore", "calls", "stores.js", "%", false),
            ("missingEvent", "references", "listener.html", "%", false),
            ("shadowStore", "calls", "stores.js", "%", false),
            ("shadowEvent", "references", "listener.html", "%", false),
            ("ordinary", "calls", "consumer.js", "helper", true),
            ("ordinary", "calls", "ordinary.js", "imported", true),
        ] {
            let count = edge_count(source, kind, path, target);
            if count != usize::from(active) {
                failures.push(format!(
                    "{source} --{kind}--> {path}:{target}: actual={count}, expected={}",
                    usize::from(active)
                ));
            }
        }
        if bindings {
            for (source, key, status) in [
                ("missingStore", "absent", "unresolved"),
                ("missingEvent", "absent", "unresolved"),
                (
                    "run",
                    if renamed { "basket" } else { "cart" },
                    if unique {
                        "resolved"
                    } else if target_state == "duplicate" {
                        "ambiguous"
                    } else {
                        "unresolved"
                    },
                ),
                (
                    "emit",
                    if renamed { "basket-open" } else { "cart-open" },
                    if unique {
                        "resolved"
                    } else if target_state == "duplicate" {
                        "ambiguous"
                    } else {
                        "unresolved"
                    },
                ),
            ] {
                let line: usize = conn.query_row("SELECT n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='consumer.js' AND n.name=?1", [source], |r|r.get(0)).unwrap();
                let statuses: Vec<String> = conn.prepare("SELECT DISTINCT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='consumer.js' AND c.line=?1 AND x.expression LIKE ?2 ORDER BY c.status").unwrap()
                    .query_map(params![line, format!("%{key}%")], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                if statuses != vec![status.to_string()] {
                    failures.push(format!(
                        "{source} named-binding coverage: actual={statuses:?}, expected={status}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "pack relations requests={requests} bindings={bindings} renamed={renamed} targets={target_state}:\n{}", failures.join("\n"));
        drop(conn);
        complete_parity(&workspace);
    };
    observe(true, true, false, "unique");
    // Provider and listener changes traverse the real delta writer, then reopen.
    for state in ["missing", "duplicate", "unique"] {
        workspace.write("stores.js", match state {
            "missing" => "// store removed\n",
            "duplicate" => "Alpine.store('cart', {checkout() { return 1; }});\nAlpine.store('cart', {checkout() { return 2; }});\n",
            _ => stores,
        });
        workspace.delta("stores.js");
        workspace.write("listener.html", match state {
            "missing" => "<p>listener removed</p>\n",
            "duplicate" => "<button x-on:cart-open='ordinary()'></button>\n<button x-on:cart-open='ordinary()'></button>\n",
            _ => listener,
        });
        workspace.delta("listener.html");
        observe(true, true, false, state);
    }
    let packs = [
        (
            "htmx",
            include_str!("../../../src/engine/languages/manifests/htmx.toml"),
        ),
        (
            "alpinejs",
            include_str!("../../../src/engine/languages/manifests/alpinejs.toml"),
        ),
    ];
    for (name, source) in packs {
        let path = format!(".forge/frameworks/{name}.toml");
        let mut pack: toml::Value = toml::from_str(source).unwrap();
        pack.as_table_mut()
            .unwrap()
            .insert("replace".into(), true.into());
        pack["syntax_rules"] = toml::Value::Array(vec![]);
        workspace.write(&path, &toml::to_string(&pack).unwrap());
        workspace.delta(&path);
        observe(name != "htmx", name != "alpinejs", false, "unique");
        fs::remove_file(workspace.0.join(&path)).unwrap();
        workspace.delta(&path);
        observe(true, true, false, "unique");
    }
    let rename = |source: &str| {
        let mut renamed = source.to_owned();
        for (old, new) in [
            ("Alpine", "Islands"),
            ("$store", "$registry"),
            ("$dispatch", "$send"),
            ("x-on:", "q-listen:"),
            ("x-data", "q-data"),
            ("hx-", "req-"),
            ("cart", "basket"),
        ] {
            renamed = renamed.replace(old, new);
        }
        renamed
    };
    for (name, source) in packs {
        let path = format!(".forge/frameworks/{name}.toml");
        let mut pack: toml::Value = toml::from_str(&rename(source)).unwrap();
        pack.as_table_mut()
            .unwrap()
            .insert("replace".into(), true.into());
        workspace.write(&path, &toml::to_string(&pack).unwrap());
        workspace.delta(&path);
    }
    // Old source spellings lose semantics; ordinary calls remain intact.
    observe(false, false, false, "unique");
    for (path, source) in fixture {
        workspace.write(path, &rename(source));
        workspace.delta(path);
    }
    observe(true, true, true, "unique");
    let isolated = Workspace::new();
    for (path, source) in fixture {
        isolated.write(path, source);
    }
    writer::build(&isolated.0, &isolated.db(), None).unwrap();
    let conn = isolated.open();
    assert_valid(&conn);
    assert_eq!(
        edge_occurrence_rows(&conn, "page.html", "calls_endpoint").len(),
        10,
        "workspace-local replacement does not escape its owner"
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='run' AND d.name='checkout' AND e.kind='calls'", [], |r|r.get::<_, usize>(0)).unwrap(), 1);
    complete_parity(&isolated);
}
