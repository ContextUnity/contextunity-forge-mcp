use super::*;
use contextunity_forge_mcp::engine::languages::html::extract_html_file;

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
        if requests {
            let item_handler_id = node_id(&conn, "backend.py", "item");
            let item_routes: Vec<String> = conn
                .prepare("SELECT s.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='handles' AND d.id=?1 ORDER BY s.id")
                .unwrap()
                .query_map([item_handler_id], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            expected.push((16, item_routes[0].clone()));
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
        11,
        "workspace-local replacement does not escape its owner"
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='run' AND d.name='checkout' AND e.kind='calls'", [], |r|r.get::<_, usize>(0)).unwrap(), 1);
    complete_parity(&isolated);
}

#[test]
fn parameterized_htmx_requests_resolve_to_backend_route_endpoints() {
    let mut probe_facts = contextunity_forge_mcp::core::models::Facts::default();
    let _ = extract_html_file("channels/settings.html", "<div></div>", "channels.settings", &mut probe_facts);

    let workspace = Workspace::new();
    let backend = r#"from django.urls import path

def channel_credentials(request): return 1
def channel_diagnostic(request): return 1
def channel_admin_probe(request): return 1
def channel_delete(request): return 1
def product_editor_images(request): return 1
def taxonomy_review_resolve(request): return 1
def technology_pane(request): return 1
def fixed_only_static(request): return 1
def star_literal(request): return 1
def version_two(request): return 1
def item_detail(request): return 1

urlpatterns = [
    path('channels/<str:provider>/<str:profile_name>/credentials/', channel_credentials),
    path('channels/<str:provider>/<str:profile_name>/diagnostic/', channel_diagnostic),
    path('channels/<str:provider>/<str:profile_name>/admin-probe/', channel_admin_probe),
    path('channels/<str:provider>/<str:profile_name>/delete/', channel_delete),
    path('products/api/<uuid:pk>/editor-images/', product_editor_images),
    path('taxonomy/reviews/<str:review_id>/resolve/', taxonomy_review_resolve),
    path('technologies/<int:id>/pane/', technology_pane),
    path('only/static/', fixed_only_static),
    path('star/*literal/', star_literal),
    path('versions/2/items/<int:pk>/', version_two),
    path('items/<int:pk>/', item_detail),
]
"#;

    // 4 channel actions in channels/settings.html
    let channels_settings = r#"<div>
        <button hx-get="/channels/{{ profile.provider }}/{{ profile.profile_name }}/credentials/"></button>
        <button hx-get="/channels/{{ profile.provider }}/{{ profile.profile_name }}/diagnostic/"></button>
        <button hx-post="/channels/{{ profile.provider }}/{{ profile.profile_name }}/admin-probe/"></button>
        <button hx-post="/channels/{{ profile.provider }}/{{ profile.profile_name }}/delete/"></button>
    </div>"#;

    // 6 image requests in products/editor/_images.html
    let products_images = r#"<div>
        <form hx-post="/products/api/{{ product.pk }}/editor-images/"></form>
        <button hx-post="/products/api/{{ product.pk }}/editor-images/"></button>
        <div hx-get="/products/api/{{ product.pk }}/editor-images/"></div>
        <form hx-get="/products/api/{{ product.pk }}/editor-images/"></form>
        <form hx-get="/products/api/{{ product.pk }}/editor-images/"></form>
        <button hx-post="/products/api/{{ product.pk }}/editor-images/"></button>
    </div>"#;

    // 3 review actions in taxonomy_review/_drawer.html
    let taxonomy_drawer = r#"<div>
        <form hx-post="/taxonomy/reviews/{{ review.pk }}/resolve/"></form>
        <form hx-post="/taxonomy/reviews/{{ review.pk }}/resolve/"></form>
        <form hx-post="/taxonomy/reviews/{{ review.pk }}/resolve/"></form>
    </div>"#;

    // 1 pane request in technologies/_set_detail.html
    let technologies_pane = r#"<div>
        <button hx-get="/technologies/{{ tech.id }}/pane/"></button>
    </div>"#;

    // Negative dynamic-to-fixed controls (lines 2, 3, 4, 5) and positive controls (lines 6, 7)
    let controls = r#"<div>
        <button hx-get="/only/{{ unknown }}/"></button>
        <button hx-get="/{{ unknown }}/static/"></button>
        <button hx-get="/star/{{ unknown }}/"></button>
        <button hx-get="/versions/1/items/{{ item.pk }}/"></button>
        <button hx-get="/versions/2/items/{{ item.pk }}/"></button>
        <button hx-get="/items/{{ item.pk }}/"></button>
    </div>"#;

    let fixture = [
        ("urls.py", backend),
        ("channels/settings.html", channels_settings),
        ("products/editor/_images.html", products_images),
        ("taxonomy_review/_drawer.html", taxonomy_drawer),
        ("technologies/_set_detail.html", technologies_pane),
        ("controls.html", controls),
    ];
    for (path, source) in fixture {
        workspace.write(path, source);
    }

    let check_state = |workspace: &Workspace, tech_count: usize, active_htmx: bool| {
        let conn = workspace.open();
        assert_valid(&conn);

        let table: [(&str, usize, &str); 14] = [
            ("channels/settings.html", 2, "channel_credentials"),
            ("channels/settings.html", 3, "channel_diagnostic"),
            ("channels/settings.html", 4, "channel_admin_probe"),
            ("channels/settings.html", 5, "channel_delete"),
            ("products/editor/_images.html", 2, "product_editor_images"),
            ("products/editor/_images.html", 3, "product_editor_images"),
            ("products/editor/_images.html", 4, "product_editor_images"),
            ("products/editor/_images.html", 5, "product_editor_images"),
            ("products/editor/_images.html", 6, "product_editor_images"),
            ("products/editor/_images.html", 7, "product_editor_images"),
            ("taxonomy_review/_drawer.html", 2, "taxonomy_review_resolve"),
            ("taxonomy_review/_drawer.html", 3, "taxonomy_review_resolve"),
            ("taxonomy_review/_drawer.html", 4, "taxonomy_review_resolve"),
            ("technologies/_set_detail.html", 2, "technology_pane"),
        ];

        if active_htmx {
            for (file, line, handler) in table {
                let handler_node = node_id(&conn, "urls.py", handler);
                let route_id: String = conn
                    .prepare(
                        "SELECT s.id FROM edge_occurrences e \
                         JOIN nodes s ON s.node_hash=e.src_hash \
                         JOIN nodes d ON d.node_hash=e.dst_hash \
                         WHERE e.kind='handles' AND d.id=?1",
                    )
                    .unwrap()
                    .query_row([handler_node], |r| r.get(0))
                    .unwrap();

                let edge_target: String = conn
                    .prepare(
                        "SELECT d.id FROM edge_occurrences e \
                         JOIN nodes d ON d.node_hash=e.dst_hash \
                         JOIN path_dictionary p ON p.path_id=e.owner_id \
                         WHERE p.path=?1 AND e.kind='calls_endpoint' AND e.line=?2",
                    )
                    .unwrap()
                    .query_row(params![file, line], |r| r.get(0))
                    .unwrap();
                assert_eq!(
                    edge_target, route_id,
                    "target route mismatch for {file}:{line} -> {handler}"
                );

                let status: String = conn
                    .prepare(
                        "SELECT DISTINCT c.status FROM resolution_coverage c \
                         JOIN path_dictionary p ON p.path_id=c.path_id \
                         WHERE p.path=?1 AND c.line=?2",
                    )
                    .unwrap()
                    .query_row(params![file, line], |r| r.get(0))
                    .unwrap();
                assert_eq!(
                    status, "resolved",
                    "coverage status mismatch for {file}:{line}"
                );
            }

            let tech_edges = edge_occurrence_rows(&conn, "technologies/_set_detail.html", "calls_endpoint");
            assert_eq!(tech_edges.len(), tech_count);

            let v2_handler = node_id(&conn, "urls.py", "version_two");
            let v2_route: String = conn
                .prepare(
                    "SELECT s.id FROM edge_occurrences e \
                     JOIN nodes s ON s.node_hash=e.src_hash \
                     JOIN nodes d ON d.node_hash=e.dst_hash \
                     WHERE e.kind='handles' AND d.id=?1",
                )
                .unwrap()
                .query_row([v2_handler], |r| r.get(0))
                .unwrap();
            let v2_target: String = conn
                .prepare(
                    "SELECT d.id FROM edge_occurrences e \
                     JOIN nodes d ON d.node_hash=e.dst_hash \
                     JOIN path_dictionary p ON p.path_id=e.owner_id \
                     WHERE p.path='controls.html' AND e.kind='calls_endpoint' AND e.line=6",
                )
                .unwrap()
                .query_row([], |r| r.get(0))
                .unwrap();
            assert_eq!(v2_target, v2_route);

            let item_handler = node_id(&conn, "urls.py", "item_detail");
            let item_route: String = conn
                .prepare(
                    "SELECT s.id FROM edge_occurrences e \
                     JOIN nodes s ON s.node_hash=e.src_hash \
                     JOIN nodes d ON d.node_hash=e.dst_hash \
                     WHERE e.kind='handles' AND d.id=?1",
                )
                .unwrap()
                .query_row([item_handler], |r| r.get(0))
                .unwrap();
            let item_target: String = conn
                .prepare(
                    "SELECT d.id FROM edge_occurrences e \
                     JOIN nodes d ON d.node_hash=e.dst_hash \
                     JOIN path_dictionary p ON p.path_id=e.owner_id \
                     WHERE p.path='controls.html' AND e.kind='calls_endpoint' AND e.line=7",
                )
                .unwrap()
                .query_row([], |r| r.get(0))
                .unwrap();
            assert_eq!(item_target, item_route);

            for neg_line in [2, 3, 4, 5] {
                let neg_edges: usize = conn
                    .prepare(
                        "SELECT count(*) FROM edge_occurrences e \
                         JOIN path_dictionary p ON p.path_id=e.owner_id \
                         WHERE p.path='controls.html' AND e.kind='calls_endpoint' AND e.line=?1",
                    )
                    .unwrap()
                    .query_row([neg_line], |r| r.get(0))
                    .unwrap();
                assert_eq!(
                    neg_edges, 0,
                    "dynamic-to-fixed control line {neg_line} must emit 0 calls_endpoint edges"
                );

                let neg_status: String = conn
                    .prepare(
                        "SELECT DISTINCT c.status FROM resolution_coverage c \
                         JOIN path_dictionary p ON p.path_id=c.path_id \
                         WHERE p.path='controls.html' AND c.line=?1",
                    )
                    .unwrap()
                    .query_row([neg_line], |r| r.get(0))
                    .unwrap();
                assert_eq!(
                    neg_status, "unresolved",
                    "dynamic-to-fixed control line {neg_line} must remain unresolved"
                );
            }
        } else {
            let total_edges: usize = conn
                .query_row(
                    "SELECT count(*) FROM edge_occurrences WHERE kind='calls_endpoint'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(total_edges, 0, "HTMX override should strip all calls_endpoint edges");
        }

        drop(conn);
        complete_parity(workspace);
    };

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    check_state(&workspace, 1, true);

    // Delta update: modify technologies/_set_detail.html
    let tech_detail_updated = r#"<div>
        <button hx-get="/technologies/{{ tech.id }}/pane/"></button>
        <button hx-get="/technologies/{{ tech.id }}/pane/"></button>
    </div>"#;
    workspace.write("technologies/_set_detail.html", tech_detail_updated);
    workspace.delta("technologies/_set_detail.html");
    check_state(&workspace, 2, true);

    // HTMX framework pack override with replace=true and empty syntax rules
    let htmx_pack_src = include_str!("../../../src/engine/languages/manifests/htmx.toml");
    let override_path = ".forge/frameworks/htmx.toml";
    let mut pack: toml::Value = toml::from_str(htmx_pack_src).unwrap();
    pack.as_table_mut().unwrap().insert("replace".into(), true.into());
    pack["syntax_rules"] = toml::Value::Array(vec![]);
    workspace.write(override_path, &toml::to_string(&pack).unwrap());
    workspace.delta(override_path);
    check_state(&workspace, 0, false);

    // Restore default HTMX pack
    fs::remove_file(workspace.0.join(override_path)).unwrap();
    workspace.delta(override_path);
    check_state(&workspace, 2, true);
}

#[test]
fn pipeline_optimization_contract_verifies_zero_alloc_and_bounded_rss() {
    let mut facts = Facts::default();
    extract_html_file("test.html", "<div hx-get='/api/test'></div>", "test", &mut facts).unwrap();
    panic!("CONTRACT_RED: pipeline optimizations not yet implemented");
}
