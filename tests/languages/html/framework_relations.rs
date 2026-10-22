use super::{commitments, reader, writer, Workspace};
use contextunity_forge_mcp::engine::ast;
use rusqlite::{params, Connection};
use std::{fs, path::PathBuf};

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

fn complete_parity(workspace: &Workspace) {
    workspace.assert_cold_equivalent();
    let incremental = workspace.open();
    let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0).unwrap();
    for sql in [
        "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
        "SELECT json_array(path,line,message) FROM errors ORDER BY path,line,message",
        "SELECT json_array(s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence) FROM edge_occurrences o JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id JOIN coverage_evidence e ON e.evidence_id=o.evidence_id JOIN coverage_evidence c ON c.evidence_id=o.confidence_id ORDER BY s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence",
        "SELECT json_array(s.id,d.id,e.kind,e.occurrence_count) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash ORDER BY s.id,d.id,e.kind",
    ] {
        let rows = |conn: &Connection| conn.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(rows(&incremental), rows(&cold), "{sql}");
    }
}

fn file_edges(workspace: &Workspace, path: &str, target: Option<(&str, usize)>) {
    let conn = workspace.open();
    assert_valid(&conn);
    let expected = target
        .map(|(target, line)| {
            (
                node_id(&conn, path, "page"),
                format!("module:{target}"),
                line,
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        edge_occurrence_rows(&conn, path, "renders_template"),
        expected,
        "{path}"
    );
}

fn node_id(conn: &Connection, path: &str, name: &str) -> String {
    let rows = conn
        .prepare("SELECT id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND name=?2 ORDER BY id")
        .unwrap()
        .query_map(params![path, name], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows.len(), 1, "unique definition {path}:{name}: {rows:?}");
    rows[0].clone()
}

fn edge_occurrence_rows(
    conn: &Connection,
    source_path: &str,
    kind: &str,
) -> Vec<(String, String, usize)> {
    conn.prepare("SELECT s.id,d.id,o.line FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash WHERE p.path=?1 AND o.kind=?2 ORDER BY o.line")
        .unwrap()
        .query_map(params![source_path, kind], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn persisted_facts(conn: &Connection) -> Vec<contextunity_forge_mcp::core::models::Facts> {
    let blobs: Vec<Vec<u8>> = conn
        .prepare("SELECT facts_blob FROM local_facts ORDER BY path")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    blobs
        .iter()
        .map(|blob| {
            serde_json::from_slice(&zstd::stream::decode_all(blob.as_slice()).unwrap()).unwrap()
        })
        .collect()
}

fn named_route_ids(conn: &Connection, name: &str) -> Vec<String> {
    conn.prepare(
        "SELECT id FROM nodes WHERE kind='route' AND json_extract(details,'$.route_name')=?1 ORDER BY id",
    )
    .unwrap()
    .query_map([name], |row| row.get(0))
    .unwrap()
    .collect::<rusqlite::Result<Vec<_>>>()
    .unwrap()
}

fn named_route_coverage(conn: &Connection, path: &str, line: usize, name: &str) -> String {
    let expression = format!("django.route_name.{name}");
    conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        params![path, line, expression],
        |row| row.get(0),
    )
    .unwrap_or_else(|error| {
        panic!("missing named-route coverage {path}:{line} {expression}: {error}")
    })
}

fn assert_valid(conn: &Connection) {
    let errors: Vec<(String, usize, String)> = conn
        .prepare("SELECT path,line,message FROM errors ORDER BY path,line,message")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(errors.is_empty(), "invalid fixture: {errors:?}");
    commitments::verify(conn).unwrap();
}

fn assert_source_valid(path: &str, language: &str, source: &str, names: &[&str]) {
    let facts = ast::extract(path, language, source).unwrap();
    assert!(
        facts.errors.is_empty(),
        "{path} must parse: {:?}",
        facts.errors
    );
    for name in names {
        assert_eq!(
            facts.nodes.iter().filter(|node| node.name == *name).count(),
            1,
            "{path} expected ordinary producer/consumer definition {name}: {:?}",
            facts.nodes
        );
    }
}

#[test]
fn two_phase_semantic_facts_reach_persisted_consumers_and_follow_provider_deltas() {
    // Corpus-shaped imported call -> actual template file. Ordinary declarations,
    // a valid import and a real target are prerequisites, never substitutes for
    // the exact persisted occurrence. The alias and shadow share a spelling.
    {
        let workspace = Workspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname='call-file-fixture'\ndependencies=['django']\n",
        );
        const SOURCE: &str = "from django.shortcuts import render as show_page\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n\ndef shadowed(request, show_page):\n    return show_page(request, 'catalog/card.html')\n\ndef local_render(request, template):\n    return template\n\ndef unrelated(request):\n    return local_render(request, 'catalog/card.html')\n\ndef starred(request, args):\n    return show_page(*args, 'catalog/card.html')\n\ndef comprehension(request, callbacks):\n    return [show_page(request, 'catalog/card.html') for show_page in callbacks]\n";
        const TARGET: &str = "app/templates/catalog/card.html";
        workspace.write("app/views.py", SOURCE);
        workspace.write(TARGET, "<p>Card</p>\n");
        let binding_cases = [
            ("nested-sibling", "def page(request):\n    def inner(show_page):\n        return show_page\n    return show_page(request, 'catalog/card.html')\n", Some(6)),
            ("module-rebound-before", "show_page = lambda *args: None\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("module-rebound-after", "def page(request):\n    return show_page(request, 'catalog/card.html')\nshow_page = lambda *args: None\n", None),
            ("local-rebound-before", "def page(request):\n    show_page = lambda *args: None\n    return show_page(request, 'catalog/card.html')\n", None),
            ("local-rebound-after", "def page(request):\n    value = show_page(request, 'catalog/card.html')\n    show_page = lambda *args: None\n    return value\n", None),
            ("lambda", "def page(request):\n    return (lambda show_page: show_page(request, 'catalog/card.html'))(request)\n", None),
            ("class", "class Local:\n    def page(self, request):\n        return show_page(request, 'catalog/card.html')\n", None),
            ("dynamic", "def page(request, name):\n    return show_page(request, name)\n", None),
            ("escaped", "def page(request):\n    return show_page(request, 'catalog/\\x63ard.html')\n", None),
            ("interpolated", "def page(request):\n    return show_page(request, f'catalog/{request}.html')\n", None),
            ("raw", "def page(request):\n    return show_page(request, r'catalog/card.html')\n", None),
            ("traversal", "def page(request):\n    return show_page(request, 'catalog/../card.html')\n", None),
            ("absolute", "def page(request):\n    return show_page(request, '/catalog/card.html')\n", None),
            ("keyword", "def page(request):\n    return show_page(request, template_name='catalog/card.html')\n", None),
            ("dictionary-expansion", "def page(request, options):\n    return show_page(request, 'catalog/card.html', **options)\n", None),
        ];
        for (label, body, _) in binding_cases {
            workspace.write(
                format!("cases/{label}.py"),
                &format!("from django.shortcuts import render as show_page\n\n{body}"),
            );
        }
        // Imported API identity and configured argument position are proved at
        // the complete persisted boundary for every equivalent binding form.
        let mut syntax_cases: Vec<(String, String, Option<usize>)> = [
            ("positive.py", "\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("comment-wrong-index.py", "\ndef page(request):\n    return show_page(\n        # ignored syntax trivia\n        'catalog/card.html',\n        request,\n    )\n", None),
            ("comment-valid-index.py", "\ndef page(request):\n    return show_page(\n        request,\n        # ignored syntax trivia\n        'catalog/card.html',\n    )\n", Some(4)),
            ("match-module.py", "\nmatch object():\n    case show_page:\n        pass\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("match-function.py", "\ndef page(request):\n    match request:\n        case show_page:\n            return show_page(request, 'catalog/card.html')\n", None),
            ("star-import.py", "from unknown_provider import *\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-alias.py", "type show_page = object\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-parameter.py", "\ndef page[show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("control-type-parameter.py", "\ndef page[Other](request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("control-global.py", "\ndef page(request):\n    global show_page\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("nonlocal.py", "\ndef outer():\n    show_page = lambda *args: None\n    def page(request):\n        nonlocal show_page\n        return show_page(request, 'catalog/card.html')\n    return page\n", None),
            ("class-fence.py", "\ndef outer():\n    show_page = lambda *args: None\n    class Host:\n        def page(self, request):\n            nonlocal show_page\n            return show_page(request, 'catalog/card.html')\n    return Host\n", None),
            ("type-alias-generic.py", "type show_page[T] = list[T]\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-alias-rhs-control.py", "type Other = show_page\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("type-param-bound.py", "\ndef page[show_page: object](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-variadic.py", "\ndef page[*show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-keyword.py", "\ndef page[**show_page](request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("type-param-bound-control.py", "\ndef page[Other: show_page](request):\n    return show_page(request, 'catalog/card.html')\n", Some(4)),
            ("outer-type-param.py", "\ndef outer[show_page]():\n    def page(request):\n        return show_page(request, 'catalog/card.html')\n    return page\n", None),
            ("sibling-type-param-control.py", "\ndef sibling[show_page](): pass\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", Some(5)),
            ("wildcard-before-explicit.py", "from unknown_provider import *\nfrom django.shortcuts import render as show_page\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n", None),
            ("comment-before-request.py", "\ndef page(request):\n    return show_page(\n        # before the real first argument\n        request,\n        'catalog/card.html', # after the real second argument\n    )\n", Some(4)),
        ].into_iter().map(|(label, body, line)| (
            label.into(), format!("from django.shortcuts import render as show_page\n{body}"), line
        )).collect();
        for (label, pattern, unchanged_import) in [
            ("capture-bare", "show_page", false),
            ("capture-sequence", "[show_page]", false),
            ("capture-tuple", "(show_page,)", false),
            ("capture-mapping-value", "{'x': show_page}", false),
            ("capture-list-rest", "[*show_page]", false),
            ("capture-mapping-rest", "{**show_page}", false),
            ("capture-as", "_ as show_page", false),
            ("capture-union", "[show_page] | (show_page,)", false),
            ("capture-class-positional", "list([show_page])", false),
            (
                "capture-class-keyword-value",
                "object(__class__=show_page)",
                false,
            ),
            ("control-class-head", "show_page()", true),
            ("control-dotted-value", "holder.show_page", true),
            ("control-keyword-label", "object(show_page=_)", true),
            ("control-dotted-map-key", "{holder.show_page: _}", true),
            ("control-wildcard", "_", true),
            ("control-wildcard-rest", "[*_]", true),
        ] {
            for local in [false, true] {
                let body = if local {
                    format!("\ndef page(request):\n    match request:\n        case {pattern}: pass\n    return show_page(request, 'catalog/card.html')\n")
                } else {
                    format!("\nmatch object():\n    case {pattern}: pass\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n")
                };
                syntax_cases.push((
                    format!("{label}-{local}.py"),
                    format!("from django.shortcuts import render as show_page\n{body}"),
                    unchanged_import.then_some(if local { 6 } else { 7 }),
                ));
            }
        }
        workspace.write("unknown_provider.py", "def show_page(*args): return None\n");
        for (label, source, _) in &syntax_cases {
            assert_source_valid(label, "python", source, &["page"]);
            workspace.write(format!("syntax/{label}"), source);
        }
        assert_source_valid(
            "app/views.py",
            "python",
            SOURCE,
            &[
                "page",
                "shadowed",
                "local_render",
                "unrelated",
                "starred",
                "comprehension",
            ],
        );
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        let conn = workspace.open();
        assert_valid(&conn);
        for (label, _, expected_line) in binding_cases {
            let path = format!("cases/{label}.py");
            let occurrences = edge_occurrence_rows(&conn, &path, "renders_template");
            let facts: i64 = conn.query_row("SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id, json_each(json_extract(n.details,'$.semantic_facts')) f WHERE p.path=?1 AND json_extract(f.value,'$.role.kind')='call_to_file'", [&path], |row| row.get(0)).unwrap();
            if let Some(line) = expected_line {
                assert_eq!(
                    occurrences,
                    [(
                        node_id(&conn, &path, "page"),
                        format!("module:{TARGET}"),
                        line
                    )],
                    "{label}"
                );
                assert_eq!(facts, 1, "{label}");
            } else {
                assert!(occurrences.is_empty(), "{label}: {occurrences:?}");
                assert_eq!(
                    facts, 0,
                    "unsupported syntax/binding must not retain a call fact: {label}"
                );
            }
        }
        for (label, _, expected_line) in syntax_cases {
            let path = format!("syntax/{label}");
            let occurrences = edge_occurrence_rows(&conn, &path, "renders_template");
            let facts: i64 = conn.query_row("SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id, json_each(json_extract(n.details,'$.semantic_facts')) f WHERE p.path=?1 AND json_extract(f.value,'$.role.kind')='call_to_file'", [&path], |row| row.get(0)).unwrap();
            let coverage: i64 = conn.query_row("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path=?1 AND c.status='resolved' AND c.evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='declarative file identity; unique proven target required')", [&path], |row| row.get(0)).unwrap();
            if let Some(line) = expected_line {
                assert_eq!(
                    occurrences,
                    [(
                        node_id(&conn, &path, "page"),
                        format!("module:{TARGET}"),
                        line
                    )],
                    "{label}"
                );
                assert_eq!(facts, 1, "{label}");
                assert_eq!(coverage, 1, "{label}");
            } else {
                assert!(
                    occurrences.is_empty(),
                    "unproven import or argument: {label}: {occurrences:?}"
                );
                assert_eq!(facts, 0, "{label}");
                assert_eq!(coverage, 0, "{label}");
            }
        }
        let source = node_id(&conn, "app/views.py", "page");
        let target: (String, String) = conn
            .query_row(
                "SELECT n.id,n.qualname FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='module'",
                params![TARGET],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("actual persisted HTML module target");
        assert_eq!(target.0, format!("module:{TARGET}"));
        assert_eq!(
            target.1, "app.templates.catalog.card.html",
            "actual target qualname"
        );
        let rows = edge_occurrence_rows(&conn, "app/views.py", "renders_template");
        let forbidden: Vec<_> = ["shadowed", "unrelated", "starred", "comprehension"]
            .map(|name| node_id(&conn, "app/views.py", name))
            .into_iter()
            .collect();
        assert!(
            rows.iter().all(|(owner, _, _)| !forbidden.contains(owner)),
            "shadowed/unrelated callees and unknown positional argument indexes must have no template edge: {rows:?}"
        );
        assert_eq!(
            rows,
            [(source, target.0, 4)],
            "imported alias must link the exact source occurrence to the actual module, with no guessed edges"
        );
        drop(conn);
        workspace.write(TARGET, "<p>Changed provider</p>\n");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(TARGET)).unwrap();
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(TARGET, "");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        const MOVED: &str = "app/templates/catalog/renamed.html";
        fs::rename(workspace.0.join(TARGET), workspace.0.join(MOVED)).unwrap();
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(TARGET), PathBuf::from(MOVED)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        let updated = SOURCE.replace("catalog/card.html", "catalog/renamed.html");
        workspace.write("app/views.py", &updated);
        workspace.delta("app/views.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((MOVED, 4)));
        fs::rename(
            workspace.0.join("app/views.py"),
            workspace.0.join("app/pages.py"),
        )
        .unwrap();
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from("app/views.py"), PathBuf::from("app/pages.py")],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
        fs::remove_file(workspace.0.join("app/pages.py")).unwrap();
        workspace.delta("app/pages.py");
        complete_parity(&workspace);
        assert!(
            edge_occurrence_rows(&workspace.open(), "app/pages.py", "renders_template").is_empty()
        );
        workspace.write("app/pages.py", &updated);
        workspace.delta("app/pages.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
        const REPLACE: &str = ".forge/frameworks/django.toml";
        workspace.write(
            REPLACE,
            "replace=true\nreceivers=[]\nbuiltins=[]\nfilters=[]\nroutes=[]\n",
        );
        workspace.delta(REPLACE);
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", None);
        fs::remove_file(workspace.0.join(REPLACE)).unwrap();
        workspace.delta(REPLACE);
        complete_parity(&workspace);
        file_edges(&workspace, "app/pages.py", Some((MOVED, 4)));
    }
    custom_file_rules();
}

#[test]
fn django_named_url_references_resolve_only_to_unique_routes() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-named-routes'\ndependencies=['django']\n",
    );
    workspace.write(
        "urls.py",
        "from django.urls import path\n\ndef product_list(): pass\ndef duplicate_a(): pass\ndef duplicate_b(): pass\nurlpatterns = [\n    path('products/', product_list, name='product-list'),\n    path('duplicate/a/', duplicate_a, name='duplicate'),\n    path('duplicate/b/', duplicate_b, name='duplicate'),\n]\n",
    );
    let consumer_source = "from django.urls import reverse\n\ndef product_url():\n    return reverse('product-list')\n\nimport django.urls as urls\nimport django.urls\ndef module_url():\n    return urls.reverse('product-list')\ndef qualified_url():\n    return django.urls.reverse('product-list')\ndef shadowed(reverse):\n    return reverse('product-list')\ndef dynamic(name):\n    return reverse(name)\nfrom urls import product_list\ndef ordinary(): return product_list()\n";
    workspace.write("consumer.py", consumer_source);
    let template_source = "<a href=\"{% url 'product-list' %}\">Products</a>\n";
    workspace.write("templates/product.html", template_source);
    workspace.write("templates/missing.html", "{% url 'missing' %}\n");
    workspace.write("templates/duplicate.html", "{% url 'duplicate' %}\n");

    let html = &contextunity_forge_mcp::engine::languages::html::HTML;
    let mut html_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::LanguageProfile::extract_file(
        html,
        "templates/product.html",
        template_source,
        "templates.product",
        &mut html_facts,
    )
    .unwrap();
    assert!(html_facts.references.iter().any(|reference| {
        reference.kind == "references_route"
            && reference.expression == "django.route_name.product-list"
    }));

    let python = &contextunity_forge_mcp::engine::languages::python::PYTHON;
    let mut python_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::LanguageProfile::extract_file(
        python,
        "consumer.py",
        consumer_source,
        "consumer",
        &mut python_facts,
    )
    .unwrap();
    assert!(python_facts.references.iter().any(|reference| {
        reference.kind == "references_route"
            && reference.expression == "django.route_name.product-list"
    }));

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |python_active: bool, template_active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let product_routes = named_route_ids(&conn, "product-list");
        assert_eq!(product_routes.len(), 1, "one named route must be indexed");
        assert_eq!(named_route_ids(&conn, "duplicate").len(), 2);
        assert_eq!(
            edge_occurrence_rows(&conn, "templates/product.html", "references_route"),
            if template_active {
                vec![(
                    "module:templates/product.html".into(),
                    product_routes[0].clone(),
                    1,
                )]
            } else {
                vec![]
            }
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "consumer.py", "references_route"),
            if python_active {
                [("product_url", 4), ("module_url", 9), ("qualified_url", 11)]
                    .into_iter()
                    .map(|(name, line)| {
                        (
                            node_id(&conn, "consumer.py", name),
                            product_routes[0].clone(),
                            line,
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            }
        );
        if python_active {
            for line in [4, 9, 11] {
                assert_eq!(
                    named_route_coverage(&conn, "consumer.py", line, "product-list"),
                    "resolved"
                );
            }
        }
        if template_active {
            for (path, name, status) in [
                ("templates/product.html", "product-list", "resolved"),
                ("templates/missing.html", "missing", "unresolved"),
                ("templates/duplicate.html", "duplicate", "ambiguous"),
            ] {
                assert_eq!(named_route_coverage(&conn, path, 1, name), status);
            }
        }
        for path in ["templates/missing.html", "templates/duplicate.html"] {
            assert!(
                edge_occurrence_rows(&conn, path, "references_route").is_empty(),
                "non-unique named route must not emit an edge: {path}"
            );
        }
        let references = persisted_facts(&conn)
            .iter()
            .flat_map(|facts| &facts.references)
            .filter(|reference| reference.kind == "references_route")
            .count();
        assert_eq!(
            references,
            3 * usize::from(python_active) + 3 * usize::from(template_active)
        );
        let coverage: usize = conn.query_row(
            "SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression LIKE 'django.route_name.%'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            coverage, references,
            "removed rules leave no semantic coverage"
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "consumer.py", "calls"),
            [(
                node_id(&conn, "consumer.py", "ordinary"),
                node_id(&conn, "urls.py", "product_list"),
                17
            )]
        );
        let import_status: String = conn.query_row(
            "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='consumer.py' AND c.line=16 AND x.expression='product_list'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            import_status, "resolved",
            "ordinary import survives rule changes"
        );
        drop(conn);
        complete_parity(&workspace);
    };
    assert_state(true, true);

    let manifest = ".forge/frameworks/django.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../src/engine/languages/manifests/django.toml"
    ))
    .unwrap();
    pack.as_table_mut()
        .unwrap()
        .insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed["syntax_rules"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| {
            !matches!(
                rule["id"].as_str(),
                Some("named-route-call" | "named-route-tag")
            )
        });
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(manifest);
    assert_state(false, false);

    let call = pack["syntax_rules"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|rule| rule["id"].as_str() == Some("named-route-call"))
        .unwrap();
    call["operation"]["module"] = "atelier.links".into();
    call["operation"]["export"] = "locate".into();
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(manifest);
    assert_state(false, true);
    workspace.write(
        "consumer.py",
        &consumer_source
            .replace("django.urls", "atelier.links")
            .replace("reverse", "locate"),
    );
    workspace.delta("consumer.py");
    assert_state(true, true);

    fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    assert_state(false, true);
    workspace.write("consumer.py", consumer_source);
    workspace.delta("consumer.py");
    assert_state(true, true);
}

#[test]
fn django_cbv_attributes_link_only_unique_templates_and_models() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-cbv'\ndependencies=['django']\n",
    );
    workspace.write(
        "app/models.py",
        "from django.db import models\nclass Product(models.Model):\n    pass\nclass Duplicate(models.Model):\n    pass\n",
    );
    workspace.write(
        "other/models.py",
        "from django.db import models\nclass Duplicate(models.Model):\n    pass\n",
    );
    let views_source = "from app.models import Product, Missing, Duplicate\nfrom other.models import Duplicate\n\nclass ProductView:\n    template_name = 'products/detail.html'\n    model = Product\n\nclass MissingTemplateView:\n    template_name = 'missing.html'\n    model = Product\n\nclass DuplicateTemplateView:\n    template_name = 'products/duplicate.html'\n    model = Product\n\nclass MissingModelView:\n    template_name = 'products/detail.html'\n    model = Missing\n\nclass DuplicateModelView:\n    template_name = 'products/detail.html'\n    model = Duplicate\n\nclass DynamicView:\n    template_name = choose_template()\n    model = choose_model()\n";
    workspace.write("app/views.py", views_source);
    workspace.write("app/templates/products/detail.html", "<p>Detail</p>\n");
    workspace.write("app/templates/products/duplicate.html", "<p>First</p>\n");
    workspace.write("other/templates/products/duplicate.html", "<p>Second</p>\n");

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = workspace.open();
    assert_valid(&conn);

    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "renders_template"),
        [
            (
                node_id(&conn, "app/views.py", "ProductView"),
                "module:app/templates/products/detail.html".into(),
                5
            ),
            (
                node_id(&conn, "app/views.py", "MissingModelView"),
                "module:app/templates/products/detail.html".into(),
                17
            ),
            (
                node_id(&conn, "app/views.py", "DuplicateModelView"),
                "module:app/templates/products/detail.html".into(),
                21
            ),
        ]
    );
    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model"),
        [
            (
                node_id(&conn, "app/views.py", "ProductView"),
                node_id(&conn, "app/models.py", "Product"),
                6
            ),
            (
                node_id(&conn, "app/views.py", "MissingTemplateView"),
                node_id(&conn, "app/models.py", "Product"),
                10
            ),
            (
                node_id(&conn, "app/views.py", "DuplicateTemplateView"),
                node_id(&conn, "app/models.py", "Product"),
                14
            ),
        ]
    );
    for (line, expression, status) in [
        (9, "'missing.html'", "unresolved"),
        (13, "'products/duplicate.html'", "ambiguous"),
        (18, "Missing", "unresolved"),
        (22, "Duplicate", "ambiguous"),
    ] {
        let actual: String = conn
            .query_row(
                "SELECT status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=?1 AND x.expression=?2",
                params![line, expression],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("missing coverage at app/views.py:{line} for {expression}: {error}"));
        assert_eq!(actual, status, "{expression}");
    }
    for (line, expression) in [(25, "choose_template()"), (26, "choose_model()")] {
        let status: String = conn
            .query_row(
                "SELECT status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=?1 AND x.expression=?2",
                params![line, expression],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "unresolved", "{expression}");
    }
    assert!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model")
            .iter()
            .all(|(source, _, _)| source != &node_id(&conn, "app/views.py", "DuplicateModelView")),
        "ambiguous duplicate model declarations must not produce an edge"
    );
}

#[test]
fn framework_manifest_class_attributes_follow_effective_rules_and_delta() {
    configured_syntax_rules_follow_effective_rules_and_delta();
    configured_template_rules_follow_effective_rules_and_delta();
    configured_di_origins_follow_effective_rules_and_delta();
    configured_component_and_receiver_rules_follow_effective_rules_and_delta();
    configured_component_macros_follow_effective_rules_and_delta();
    configured_router_prefixes_are_static_unique_and_recursive();
    for (pack_name, file_attribute, symbol_attribute, directory, module, export) in [
        (
            "kiln",
            "panel_asset",
            "entity_type",
            "screens/",
            "kiln.paint",
            "emit",
        ),
        (
            "orchard",
            "markup_asset",
            "record_kind",
            "presentations/",
            "orchard.ink",
            "draw",
        ),
    ] {
        let workspace = Workspace::new();
        let manifest_path = format!(".forge/frameworks/{pack_name}.toml");
        let project = "[project]\nname='attribute-rules'\n";
        workspace.write("pyproject.toml", project);
        workspace.write(
            "app/models.py",
            "class Record: pass\nclass Duplicate: pass\n",
        );
        workspace.write("other/models.py", "class Duplicate: pass\n");
        let source = format!(
            "from app.models import Record, Missing, Duplicate\nfrom other.models import Duplicate\nfrom {module} import {export} as paint\n\nclass Unique:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Record\n\nclass MissingFile:\n    {file_attribute} = 'absent.html'\n    {symbol_attribute} = Record\n\nclass DuplicateFile:\n    {file_attribute} = 'catalog/duplicate.html'\n    {symbol_attribute} = Record\n\nclass MissingSymbol:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Missing\n\nclass DuplicateSymbol:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Duplicate\n\nclass Dynamic:\n    {file_attribute} = choose_asset()\n    {symbol_attribute} = choose_type()\n\ndef helper(): return 1\ndef ordinary(): return helper()\ndef page(): return paint('catalog/card.html')\ndef shadowed(paint): return paint('catalog/card.html')\n"
        );
        workspace.write("app/views.py", &source);
        let target = format!("app/{directory}catalog/card.html");
        workspace.write(&target, "<p>Unique</p>\n");
        for owner in ["app", "other"] {
            workspace.write(
                format!("{owner}/{directory}catalog/duplicate.html"),
                "<p>Duplicate</p>\n",
            );
        }
        // A suffix within a directory name is not an eligible path segment.
        workspace.write(format!("app/not{directory}absent.html"), "<p>Decoy</p>\n");
        let mut pack = serde_json::json!({
            "receivers": [], "builtins": [], "filters": [], "routes": [],
            "semantic_rules": [
                {"id":"asset", "namespace":"unrelated.assets", "scope":"project", "activation":[], "operation":{"kind":"class_attribute_to_file", "language":"python", "attribute":file_attribute, "target":{"kind":"project_path_suffix", "prefix":directory, "language":"html"}, "relation":"renders_template"}},
                {"id":"entity", "namespace":"unrelated.entities", "scope":"project", "activation":[], "operation":{"kind":"class_attribute_to_symbol", "language":"python", "attribute":symbol_attribute, "target":{"kind":"python_symbol", "language":"python"}, "relation":"uses_model"}},
                {"id":"paint", "namespace":"unrelated.assets", "scope":"project", "activation":[], "operation":{"kind":"call_to_file", "language":"python", "callee":{"kind":"imported", "module":module, "export":export}, "argument":0, "target":{"kind":"project_path_suffix", "prefix":directory, "language":"html"}, "relation":"renders_template"}}
            ]
        });
        let write_pack = |value: &serde_json::Value| {
            workspace.write(&manifest_path, &toml::to_string(value).unwrap());
        };
        write_pack(&pack);
        writer::build(&workspace.0, &workspace.db(), None).unwrap_or_else(|error| {
            panic!("configured {pack_name} attributes and directory must build: {error:#}")
        });
        let assert_relations = |active: bool| {
            let conn = workspace.open();
            assert_valid(&conn);
            for (kind, owners, destination) in [
                (
                    "renders_template",
                    vec!["Unique", "MissingSymbol", "DuplicateSymbol", "page"],
                    format!("module:{target}"),
                ),
                (
                    "uses_model",
                    vec!["Unique", "MissingFile", "DuplicateFile"],
                    node_id(&conn, "app/models.py", "Record"),
                ),
            ] {
                let mut expected: Vec<_> = if active {
                    owners
                        .iter()
                        .map(|name| (node_id(&conn, "app/views.py", name), destination.clone()))
                        .collect()
                } else {
                    vec![]
                };
                expected.sort();
                let mut actual: Vec<_> = edge_occurrence_rows(&conn, "app/views.py", kind)
                    .into_iter()
                    .map(|(source, target, _)| (source, target))
                    .collect();
                actual.sort();
                assert_eq!(actual, expected, "{pack_name}: {kind}, active={active}");
            }
            let ordinary_calls: i64 = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                ordinary_calls, 1,
                "ordinary language calls survive rule changes"
            );
            let imports: i64 = conn.query_row(
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=1 AND x.expression='Record' AND c.status='resolved'",
                [], |row| row.get(0),
            ).unwrap();
            assert!(imports > 0, "ordinary import facts survive rule changes");
            if active {
                for (expression, status) in [
                    ("'absent.html'", "unresolved"),
                    ("'catalog/duplicate.html'", "ambiguous"),
                    ("Missing", "unresolved"),
                    ("Duplicate", "ambiguous"),
                    ("choose_asset()", "unresolved"),
                    ("choose_type()", "unresolved"),
                ] {
                    let statuses: Vec<String> = conn.prepare(
                        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND x.expression=?1",
                    ).unwrap().query_map([expression], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
                    assert!(
                        statuses.iter().any(|actual| actual == status),
                        "{pack_name}: {expression} requires {status}: {statuses:?}"
                    );
                }
            }
        };
        assert_relations(true);
        complete_parity(&workspace);

        // Only effective rules determine semantic behavior, including when
        // the language source and ordinary imports/calls remain untouched.
        for operation in pack["semantic_rules"].as_array_mut().unwrap() {
            operation["activation"] =
                serde_json::json!([{"kind":"package", "name":"unrelated-engine"}]);
        }
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        workspace.write(
            "pyproject.toml",
            "[project]\nname='attribute-rules'\ndependencies=['unrelated-engine']\n",
        );
        workspace.delta("pyproject.toml");
        assert_relations(true);
        complete_parity(&workspace);

        let mut replacement = pack.clone();
        replacement["replace"] = true.into();
        replacement["semantic_rules"] = serde_json::json!([]);
        write_pack(&replacement);
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(true);
        complete_parity(&workspace);
        fs::remove_file(workspace.0.join(&manifest_path)).unwrap();
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(true);
        complete_parity(&workspace);
    }
}

fn configured_router_prefixes_are_static_unique_and_recursive() {
    let workspace = Workspace::new();
    workspace.write("pyproject.toml", "[project]\nname='prefix-proof'\n");
    let pack = serde_json::json!({
        "receivers":[], "builtins":[], "filters":[], "routes":[],
        "route_decorators":[{"callee":".expose", "method":"GET", "path_argument":0,
            "receiver_names":["leaf"], "prefixes":[
                {"kind":"constructor","callee":"Mount","keyword":"base_path"},
                {"kind":"attachment","callee":".attach","child_argument":0,"prefix_keyword":"under"}
            ]}],
        "syntax_rules":[{"id":"client","language":"python","operation":{
            "kind":"http_client_call","callee":"ask","receivers":[],"methods":{},
            "default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"]}}]
    });
    let manifest = ".forge/frameworks/prefix-proof.toml";
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    let valid = "root = Mount(base_path='/outer')\nmiddle = Mount(base_path='/middle')\nleaf = Mount(base_path='/leaf')\nroot.attach(middle, under='/v1')\nmiddle.attach(leaf, under='/v2')\n@leaf.expose('/item')\ndef endpoint(): return 1\ndef consumer(): return ask('/outer/v1/middle/v2/leaf/item')\ndef helper(): return 1\ndef ordinary(): return helper()\n";
    workspace.write("app.py", valid);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_routes = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let routes: Vec<String> = conn.prepare("SELECT json_extract(details,'$.route_path') FROM nodes WHERE kind='route' ORDER BY id").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            routes,
            if active {
                vec!["/outer/v1/middle/v2/leaf/item"]
            } else {
                vec![]
            }
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "app.py", "calls_endpoint").len(),
            usize::from(active)
        );
        let ordinary: i64 = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(ordinary, 1);
    };
    assert_routes(true);
    complete_parity(&workspace);
    for suffix in [
        "def unrelated(leaf):\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated():\n    leaf = object()\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated(root, leaf):\n    root.attach(leaf, under='/shadow')\n",
        "def unrelated():\n    root = object()\n    leaf = object()\n    root.attach(leaf, under='/shadow')\n",
    ] {
        workspace.write("app.py", &format!("{valid}\n{suffix}"));
        workspace.delta("app.py");
        assert_routes(true);
        complete_parity(&workspace);
    }
    workspace.write("app.py", &format!("{valid}\ndef independent():\n    leaf = Mount(base_path='/local')\n    @leaf.expose('/own')\n    def local_endpoint(): return 1\n    return local_endpoint\n"));
    workspace.delta("app.py");
    {
        let conn = workspace.open();
        assert_valid(&conn);
        let routes: Vec<String> = conn.prepare("SELECT json_extract(details,'$.route_path') FROM nodes WHERE kind='route' ORDER BY json_extract(details,'$.route_path')").unwrap().query_map([],|row|row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            routes,
            ["/local/own", "/outer/v1/middle/v2/leaf/item"],
            "independent constructor keeps its own lexical prefix"
        );
    }
    complete_parity(&workspace);
    for invalid in [
        valid.replace("base_path='/leaf'", "base_path=unknown"),
        valid.replace("under='/v2'", "under=unknown"),
        format!("{valid}\nleaf = Mount(base_path='/rewritten')\n"),
        format!("{valid}\nleaf += object()\n"),
        format!("{valid}\ndel leaf\n"),
        format!("{valid}\nleaf, other = (object(), object())\n"),
        format!("{valid}\nroot.attach(leaf, under='/duplicate')\n"),
        format!("{valid}\nleaf.attach(root, under='/cycle')\n"),
    ] {
        workspace.write("app.py", &invalid);
        workspace.delta("app.py");
        assert_routes(false);
        complete_parity(&workspace);
    }
    workspace.write("app.py", valid);
    workspace.delta("app.py");
    assert_routes(true);
    let mut replacement = pack.clone();
    replacement["replace"] = true.into();
    replacement["route_decorators"] = serde_json::json!([]);
    replacement["syntax_rules"] = serde_json::json!([]);
    workspace.write(manifest, &toml::to_string(&replacement).unwrap());
    workspace.delta(manifest);
    assert_routes(false);
    complete_parity(&workspace);
}

fn configured_di_origins_follow_effective_rules_and_delta() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='di-origins'\ndependencies=['atelier']\n",
    );
    let source = |callee: &str| {
        format!("from atelier import {callee}\ndef provider(): return 1\ndef endpoint(): return {callee}(provider)\ndef shadow({callee}): return {callee}(provider)\ndef helper(): return 1\ndef ordinary(): return helper()\n")
    };
    workspace.write("app.py", &source("Wire"));
    let mut pack = serde_json::json!({"receivers":[],"builtins":[],"filters":[],"routes":[],"di_calls":[{"callee":"Wire","argument":0}],"semantic_rules":[{"id":"inject","namespace":"atelier","scope":"project","activation":[{"kind":"package","name":"atelier"}],"operation":{"kind":"call_to_file","language":"python","callee":{"kind":"imported","module":"atelier","export":"Wire"},"argument":0,"target":{"kind":"imported_symbol","language":"python"},"relation":"injects_dependency"}}]});
    pack["semantic_rules"][0]["operation"]["kind"] = "call_to_symbol".into();
    let first = ".forge/frameworks/di-alpha.toml";
    let second = ".forge/frameworks/di-beta.toml";
    let write = |path: &str, value: &serde_json::Value| {
        workspace.write(path, &toml::to_string(value).unwrap())
    };
    write(first, &pack);
    write(second, &pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |packs: &[&str], active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let endpoint = node_id(&conn, "app.py", "endpoint");
        let cached = persisted_facts(&conn);
        let origins: std::collections::BTreeSet<_> = cached
            .iter()
            .flat_map(|facts| &facts.references)
            .filter(|reference| reference.kind == "injects" && reference.source == endpoint)
            .map(|reference| match &reference.receiver_hint {
                Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference {
                    origin,
                }) => {
                    assert!(!origin.rule.is_empty());
                    origin.pack.as_str()
                }
                other => panic!("DI fact lost actual rule origin: {other:?}"),
            })
            .collect();
        assert_eq!(
            origins,
            packs.iter().copied().collect(),
            "same-callee packs retain independent durable identity"
        );
        for (owner, expected) in [("endpoint", usize::from(active)), ("shadow", 0)] {
            let edges:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='injects_dependency' AND s.name=?1 AND d.name='provider'", [owner], |row|row.get(0)).unwrap();
            assert_eq!(edges, expected, "DI lexical proof: {owner}");
        }
        assert_eq!(
            edge_occurrence_rows(&conn, "app.py", "calls")
                .iter()
                .filter(|(_, target, _)| *target == node_id(&conn, "app.py", "helper"))
                .count(),
            1,
            "ordinary calls survive DI rule transitions"
        );
    };
    assert_state(&["di-alpha", "di-beta"], true);
    complete_parity(&workspace);
    let renamed = ".forge/frameworks/di-renamed.toml";
    let renamed_second = ".forge/frameworks/di-beta-current.toml";
    let stale: std::collections::BTreeMap<_, _> = persisted_facts(&workspace.open())
        .into_iter()
        .map(|facts| {
            let path = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module")
                .unwrap()
                .path
                .clone();
            (path, facts)
        })
        .collect();
    fs::rename(workspace.0.join(first), workspace.0.join(renamed)).unwrap();
    fs::rename(workspace.0.join(second), workspace.0.join(renamed_second)).unwrap();
    let registry =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(&workspace.0),
        );
    let replay = contextunity_forge_mcp::engine::linker::link_with_registry(
        &stale,
        None,
        Some(&workspace.0),
        &registry,
    );
    assert!(!replay.edges.iter().any(|edge| matches!(edge.kind.as_str(), "injects" | "injects_dependency")), "both current consumers reject persisted origins from removed packs despite identical active callees");
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[
            PathBuf::from(first),
            PathBuf::from(renamed),
            PathBuf::from(second),
            PathBuf::from(renamed_second),
        ],
    )
    .unwrap();
    assert_state(&["di-renamed", "di-beta-current"], true);
    complete_parity(&workspace);
    let second = renamed_second;
    fs::remove_file(workspace.0.join(second)).unwrap();
    workspace.delta(second);
    assert_state(&["di-renamed"], true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["di_calls"] = serde_json::json!([]);
    empty["semantic_rules"] = serde_json::json!([]);
    write(renamed, &empty);
    workspace.delta(renamed);
    assert_state(&[], false);
    complete_parity(&workspace);
    pack["di_calls"][0]["callee"] = "Select".into();
    pack["semantic_rules"][0]["operation"]["callee"]["export"] = "Select".into();
    write(renamed, &pack);
    workspace.write("app.py", &source("Select"));
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(renamed), PathBuf::from("app.py")],
    )
    .unwrap();
    assert_state(&["di-renamed"], true);
    complete_parity(&workspace);
    pack["semantic_rules"][0]["activation"] =
        serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(renamed, &pack);
    workspace.delta(renamed);
    assert_state(&["di-renamed"], false);
    complete_parity(&workspace);
}

fn configured_template_rules_follow_effective_rules_and_delta() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='template-proof'\ndependencies=['atelier']\n",
    );
    workspace.write("app/models.py", "class Record: pass\n");
    workspace.write("app/views.py", "from atelier.views import emit\nfrom atelier.paths import lookup\nfrom atelier.router import Table\nfrom app.models import Record\nregistry = Table()\nregistry.add('configured/', Record, alias='configured')\ndef page(): return emit('screens/card.html', {'headline':'Title'})\ndef route(): return lookup('configured')\ndef helper(): return 1\ndef ordinary(): return helper()\n");
    workspace.write("app/environment.py", "from pathlib import Path\nfrom atelier.templates import Engine, Reader\nROOT = Path(__file__).parent / 'screens'\nengine = Engine(source=Reader(str(ROOT)))\n");
    workspace.write("app/screens/card.html", "<h1>{{ headline }}</h1>\n{% pull 'part.html' %}\n{% address 'configured' %}\n{% assign local = 'value' %}\n{{ local }}\n{% use custom %}\n{% shout 'hello' %}\n{{ 'hello'|caps }}\n");
    workspace.write("app/screens/part.html", "<p>{{ headline }}</p>\n");
    workspace.write(
        "app/templates/default.html",
        "{% include 'part.html' %}\n{% set standard = 'value' %}\n{{ standard }}\n",
    );
    workspace.write("app/templates/part.html", "<p>Default</p>\n");
    workspace.write("app/libraries/custom.py", "from atelier.tags import Library\nregistry = Library()\n@registry.command(label='shout')\ndef speak(value): return value\n@registry.decorate(label='caps')\ndef capitalize(value): return value\n");
    let pack = serde_json::json!({
        "receivers":[],"builtins":[],"filters":[],"routes":[],
        "route_calls":[{"callee":".add","method":"ANY","path_argument":0,"handler_argument":1,"receiver_names":["registry"],"name_keyword":"alias","registration_guard":"register","allow_relative_path":true}],
        "syntax_rules":[
            {"id":"register","language":"python","operation":{"kind":"receiver_registration","module":"atelier.router","constructors":["Table"],"member":"add","path_argument":0,"handler_argument":1,"name_keyword":"alias"}},
            {"id":"render","language":"python","operation":{"kind":"render_context","package":"atelier","module":"atelier.views","export":"emit","target_argument":0,"context_argument":1,"argument_count":2}},
            {"id":"loader","language":"python","operation":{"kind":"template_loader","package":"atelier","module":"atelier.templates","environment":"Engine","loader":"Reader","loader_keyword":"source"}},
            {"id":"library","language":"python","operation":{"kind":"template_library","directory":"libraries","module":"atelier.tags","constructor":"Library","members":{"command":"tag","decorate":"filter"},"name_keyword":"label"}},
            {"id":"named-call","language":"python","operation":{"kind":"named_route_call","module":"atelier.paths","export":"lookup","argument":0,"expression_prefix":"route-key:"}},
            {"id":"named-tag","language":"html","operation":{"kind":"template_named_route","tag":"address","expression_prefix":"route-key:"}},
            {"id":"symbols","language":"html","operation":{"kind":"template_symbols","package":"atelier","requires_loader":true}},
            {"id":"dialect","language":"html","operation":{"kind":"template_dialect","dialect":{"directory":"screens","assignment":"assign","function":"function","import":"fetch","from":"using","alias":"called","load":"use","include":"pull","extends":"inherit","scope_open":["when"],"scope_close":["endwhen"]}}}
        ]
    });
    let manifest = ".forge/frameworks/atelier-templates.toml";
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_active = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        for (label,sql,expected) in [
            ("import-proven receiver registration", "SELECT count(*) FROM nodes WHERE kind='route' AND json_extract(details,'$.route_name')='configured'",1),
            ("call and tag consume typed route keys", "SELECT count(*) FROM edge_occurrences WHERE kind='references_route'",2),
            ("configured include", "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE e.kind='includes' AND p.path='app/screens/card.html'",1),
            ("configured template library", "SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='references' AND d.name='speak'",1),
            ("configured template filter", "SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='references' AND d.name='capitalize'",1),
        ] {
            let actual:usize=conn.query_row(sql,[],|row|row.get(0)).unwrap();
            assert_eq!(actual,if active{expected}else{0},"{label}, active={active}");
        }
        if active {
            for (path, expression) in [
                ("app/screens/card.html", "template.variable.headline"),
                ("app/screens/part.html", "template.variable.headline"),
                ("app/screens/card.html", "template.variable.local"),
            ] {
                let status:String=conn.query_row("SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2",params![path,expression],|row|row.get(0)).unwrap();
                assert_eq!(
                    status, "resolved",
                    "render context and configured dialect {path}:{expression}"
                );
            }
        }
        let ordinary:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(ordinary, 1);
        assert_eq!(conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id JOIN path_dictionary q ON q.path_id=d.path_id WHERE e.kind='includes' AND p.path='app/templates/default.html' AND q.path='app/templates/part.html'", [], |row|row.get::<_, usize>(0)).unwrap(), 1, "bundled and configured dialects coexist");
    };
    assert_active(true);
    complete_parity(&workspace);
    let late_manifest = ".forge/frameworks/zcustom.toml";
    fs::rename(workspace.0.join(manifest), workspace.0.join(late_manifest)).unwrap();
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(manifest), PathBuf::from(late_manifest)],
    )
    .unwrap();
    assert_active(true);
    complete_parity(&workspace);
    let cached = persisted_facts(&workspace.open());
    assert!(cached.iter().flat_map(|facts| &facts.references).any(|reference| reference.kind == "includes" && reference.expression == "part.html" && matches!(&reference.receiver_hint, Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack == "zcustom" && origin.rule == "dialect")), "late pack retains actual typed include origin");
    fs::rename(workspace.0.join(late_manifest), workspace.0.join(manifest)).unwrap();
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(manifest), PathBuf::from(late_manifest)],
    )
    .unwrap();
    assert_active(true);
    let mut conflicting = pack.clone();
    conflicting["syntax_rules"] = serde_json::json!([pack["syntax_rules"][7].clone()]);
    conflicting["route_calls"] = serde_json::json!([]);
    conflicting["syntax_rules"][0]["id"] = "conflict".into();
    conflicting["syntax_rules"][0]["operation"]["dialect"]["include"] = "different".into();
    let conflict_path = ".forge/frameworks/ambiguous-dialect.toml";
    workspace.write(conflict_path, &toml::to_string(&conflicting).unwrap());
    workspace.delta(conflict_path);
    assert_eq!(workspace.open().query_row("SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='app/screens/card.html' AND n.name='local'", [], |row|row.get::<_, usize>(0)).unwrap(), 0, "conflicting directory-owning dialects fail closed");
    complete_parity(&workspace);
    fs::remove_file(workspace.0.join(conflict_path)).unwrap();
    workspace.delta(conflict_path);
    assert_active(true);
    complete_parity(&workspace);
    let card_source = fs::read_to_string(workspace.0.join("app/screens/card.html")).unwrap();
    let mut renamed_dialect = pack.clone();
    renamed_dialect["syntax_rules"][7]["operation"]["dialect"]["include"] = "draw".into();
    write(&renamed_dialect);
    workspace.delta(manifest);
    assert!(
        edge_occurrence_rows(&workspace.open(), "app/screens/card.html", "includes").is_empty(),
        "removed configured directive cannot retain an exact include"
    );
    complete_parity(&workspace);
    workspace.write(
        "app/screens/card.html",
        &card_source.replace("{% pull ", "{% draw "),
    );
    workspace.delta("app/screens/card.html");
    assert_active(true);
    complete_parity(&workspace);
    write(&pack);
    workspace.write("app/screens/card.html", &card_source);
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[
            PathBuf::from(manifest),
            PathBuf::from("app/screens/card.html"),
        ],
    )
    .unwrap();
    assert_active(true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    empty["route_calls"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    complete_parity(&workspace);
}

fn configured_component_macros_follow_effective_rules_and_delta() {
    let workspace = Workspace::new();
    let manifest = ".forge/frameworks/atelier-macros.toml";
    let source = |name: &str| {
        format!("<script setup lang=\"ts\">\nconst input = {name}<{{title: string}}>();\nconst send = publishSignal<{{(event: 'ready'): void}}>();\ndeclareOutlet<{{default(props: {{item: string}}): unknown}}>();\nsend('ready');\n</script><template><p>{{{{input.title}}}}</p></template>\n")
    };
    workspace.write("macro.vue", &source("inspectInput"));
    workspace.write("shadow.vue", "<script setup lang=\"ts\">\nfunction inspectInput() { return {title:'local'}; }\nconst input = inspectInput();\n</script><template><p>{{input.title}}</p></template>\n");
    let pack = serde_json::json!({"receivers":[],"builtins":[],"filters":[],"routes":[],"syntax_rules":[{"id":"macros","language":"vue","operation":{"kind":"component_macros","props":"inspectInput","emits":"publishSignal","slots":"declareOutlet","markers":["markComponent"]}}]});
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |props: bool, other: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let cached = persisted_facts(&conn);
        let macros: Vec<_> = cached
            .iter()
            .flat_map(|facts| &facts.nodes)
            .filter(|node| node.path == "macro.vue")
            .flat_map(|node| {
                node.details["value_flow"]["component_macros"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .collect();
        assert_eq!(
            macros.len(),
            usize::from(props) + 2 * usize::from(other),
            "durable origins follow effective macro roles"
        );
        assert!(macros
            .iter()
            .all(|fact| fact["origin"]["pack"] == "atelier-macros"
                && fact["origin"]["rule"] == "macros"));
        let fields:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='macro.vue' AND e.kind='references' AND d.kind='field' AND d.name='title'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            fields,
            usize::from(props),
            "configured typed prop targets its canonical field"
        );
        let emits:usize=conn.query_row("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='macro.vue' AND x.expression='send' AND c.status='external'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            emits,
            usize::from(other),
            "configured emits role owns callable admission"
        );
        let ordinary:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='shadow.vue' AND e.kind='calls' AND d.name='inspectInput'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            ordinary, 1,
            "same-name local call retains ordinary lexical target"
        );
        assert!(!cached
            .iter()
            .flat_map(|facts| &facts.nodes)
            .filter(|node| node.path == "shadow.vue")
            .any(|node| node.details["value_flow"]["component_macros"]
                .as_array()
                .is_some_and(|facts| !facts.is_empty())));
    };
    assert_state(true, true);
    complete_parity(&workspace);
    for mutation in [
        "inspectInput = local; publishSignal = local; declareOutlet = local;",
        "inspectInput += local; publishSignal += local; declareOutlet += local;",
        "inspectInput++; publishSignal++; declareOutlet++;",
    ] {
        let changed = source("inspectInput").replacen(
            "const input",
            &format!("function local() {{ return {{title:'local'}}; }}\n{mutation}\nconst input"),
            1,
        );
        workspace.write("macro.vue", &changed);
        workspace.delta("macro.vue");
        assert_state(false, false);
        let cached = persisted_facts(&workspace.open());
        assert!(
            !cached
                .iter()
                .flat_map(|facts| &facts.nodes)
                .filter(|node| node.path == "macro.vue")
                .any(|node| node.details["value_flow"]["vue_slots"]
                    .as_array()
                    .is_some_and(|slots| !slots.is_empty())),
            "producer writes invalidate derived slot facts: {mutation}"
        );
        complete_parity(&workspace);
    }
    workspace.write("macro.vue", &source("inspectInput"));
    workspace.delta("macro.vue");
    assert_state(true, true);
    let mut renamed = pack.clone();
    renamed["syntax_rules"][0]["operation"]["props"] = "readInput".into();
    write(&renamed);
    workspace.delta(manifest);
    assert_state(false, true);
    complete_parity(&workspace);
    workspace.write("macro.vue", &source("readInput"));
    workspace.delta("macro.vue");
    assert_state(true, true);
    complete_parity(&workspace);
    let mut inactive = renamed.clone();
    inactive["syntax_rules"][0]["activation"] =
        serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(&inactive);
    workspace.delta(manifest);
    assert_state(false, false);
    complete_parity(&workspace);
    let mut empty = renamed.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_state(false, false);
    complete_parity(&workspace);
    write(&renamed);
    workspace.delta(manifest);
    assert_state(true, true);
    complete_parity(&workspace);
}

fn configured_component_and_receiver_rules_follow_effective_rules_and_delta() {
    let workspace = Workspace::new();
    workspace.write(
        "package.json",
        r#"{"name":"configured-components","dependencies":{"@atelier/ui-kit":"*"}}"#,
    );
    workspace.write(
        "atelier.config.ts",
        "export default configureAtelier({widgets:true, addons:[]});\n",
    );
    workspace.write("widgets/CardView.vue", "<template><p>Card</p></template>\n");
    workspace.write("registered.vue", "<script>\nimport Card from './widgets/CardView.vue';\nimport { wrapWidget } from '@atelier/ui-kit';\nexport default wrapWidget({items:{ConfiguredCard: Card}});\n</script><template><ConfiguredCard/></template>\n");
    workspace.write("bare.vue", "<script>\nimport Card from './widgets/CardView.vue';\nexport default {items:{ConfiguredCard: Card}};\n</script><template><ConfiguredCard/></template>\n");
    workspace.write(
        "page.vue",
        "<template><CardView/><LateView/><ConfiguredShell/></template>\n",
    );
    workspace.write("app.ts","import { CardView } from '#widgets';\nfunction page() { return configuredRef(1); }\nfunction callback(event: ConfiguredEvent) { return event.payload.get('key'); }\nfunction implicit() { return $signal(); }\nfunction helper() { return 1; }\nfunction ordinary() { return helper(); }\nfunction shadow(configuredRef: () => number) { return configuredRef(); }\n");
    workspace.write(
        "shadow.ts",
        "function configuredRef() { return 1; }\nfunction ordinary() { return configuredRef(); }\n",
    );
    workspace.write(
        "factory.js",
        "Widget.bind('panel', {start() {this.finish();},finish() {}});\n",
    );
    let pack = serde_json::json!({"receivers":["$signal"],"builtins":[],"filters":[],"routes":[],"syntax_rules":[
        {"id":"components","language":"typescript","operation":{"kind":"component_environment","package":"@atelier/ui-kit","config_file":"atelier.config.ts","config_callee":"configureAtelier","enabled_key":"widgets","modules_key":"addons","directory":"widgets","extension":".vue","virtual_module":"#widgets","autoimports":["configuredRef"],"builtins":["ConfiguredShell"],"modules":{}}},
        {"id":"options","language":"vue","operation":{"kind":"component_options","module":"@atelier/ui-kit","export":"wrapWidget","components_key":"items"}},
        {"id":"receiver","language":"typescript","operation":{"kind":"receiver_type","receiver":{"name":"ConfiguredEvent","members":["payload"],"properties":{"payload":"Headers"},"callback_callees":["configuredCallback"],"generic":false}}},
        {"id":"factory","language":"javascript","operation":{"kind":"object_factory","callees":[".bind"],"unshadowed_globals":["Widget"]}},
        {"id":"globals","language":"javascript","operation":{"kind":"implicit_builtins","names":["Widget","Widget.bind"]}},
        {"id":"implicit","language":"typescript","operation":{"kind":"implicit_receivers","package":"@atelier/ui-kit"}}
    ]});
    let manifest = ".forge/frameworks/atelier-components.toml";
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_active = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        for (path, expression) in [
            ("page.vue", "CardView"),
            ("page.vue", "ConfiguredShell"),
            ("app.ts", "CardView"),
            ("app.ts", "configuredRef"),
            ("app.ts", "event.payload.get"),
            ("app.ts", "$signal"),
        ] {
            let statuses:Vec<String>=conn.prepare("SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.line").unwrap().query_map(params![path,expression],|row|row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert!(!statuses.is_empty(), "coverage {path}:{expression}");
            if path == "app.ts" && expression == "CardView" {
                assert_eq!(
                    statuses,
                    vec![if active { "resolved" } else { "external" }],
                    "ordinary virtual-module import survives removal"
                );
                continue;
            }
            let admitted = statuses
                .iter()
                .any(|status| status == "resolved" || status == "external");
            assert_eq!(
                admitted, active,
                "configured components/receiver {path}:{expression}: {statuses:?}"
            );
        }
        let shadow:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE e.kind='calls' AND p.path='shadow.ts' AND s.name='ordinary' AND d.name='configuredRef'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            shadow, 1,
            "configured builtin spelling preserves local declaration calls"
        );
        let same_object:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='start' AND d.name='finish'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            same_object,
            usize::from(active),
            "configured object factory owns this member proof"
        );
        let cached = persisted_facts(&conn);
        let origins = cached
            .iter()
            .flat_map(|facts| &facts.nodes)
            .filter(|node| matches!(node.name.as_str(), "start" | "finish"))
            .filter(|node| {
                let ownership = &node.details["value_flow"]["object_ownership"];
                ownership["origin"]["pack"] == "atelier-components"
                    && ownership["origin"]["rule"] == "factory"
                    && ownership["guard"] == "Widget"
            })
            .count();
        assert_eq!(
            origins,
            if active { 2 } else { 0 },
            "factory origin survives the durable fact cache and disappears with its rule"
        );
        let configurations = cached
            .iter()
            .flat_map(|facts| &facts.nodes)
            .flat_map(|node| {
                node.details["value_flow"]["component_configurations"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter(|fact| {
                fact["origin"]["pack"] == "atelier-components"
                    && fact["origin"]["rule"] == "components"
                    && fact["enabled"] == true
            })
            .count();
        assert_eq!(
            configurations,
            usize::from(active),
            "component configuration origin survives the durable cache"
        );
        let registrations=cached.iter().flat_map(|facts| &facts.references).filter(|reference| reference.kind=="references" && reference.expression=="Card").filter(|reference| matches!(&reference.receiver_hint,Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack=="atelier-components" && origin.rule=="options")).count();
        assert_eq!(
            registrations,
            if active { 2 } else { 0 },
            "options references retain actual rule origin and canonical imported alias"
        );
        for path in ["registered.vue", "bare.vue"] {
            let references = edge_occurrence_rows(&conn, path, "references");
            assert_eq!(
                references.len(),
                usize::from(active),
                "configured options own exact component reference: {path}"
            );
            if active {
                let target:String=conn.query_row("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='widgets/CardView.vue' AND n.kind='module'",[],|row|row.get(0)).unwrap();
                assert_eq!(
                    references[0].1, target,
                    "registered alias targets its imported component module"
                );
            }
        }
    };
    assert_active(true);
    complete_parity(&workspace);
    for enabled in [true, false] {
        workspace.write(
            "atelier.config.ts",
            &format!("export default configureAtelier({{widgets:{enabled}, addons:[]}});\n"),
        );
        workspace.delta("atelier.config.ts");
        let frozen: Vec<_> = [manifest, "atelier.config.ts", "package.json", "page.vue"]
            .into_iter()
            .map(|path| (path, fs::read(workspace.0.join(path)).unwrap()))
            .collect();
        for content in [
            Some("<template><p>Added</p></template>\n"),
            Some("<template><p>Changed</p></template>\n"),
            None,
        ] {
            let provider = "widgets/LateView.vue";
            if let Some(content) = content {
                workspace.write(provider, content);
            } else {
                fs::remove_file(workspace.0.join(provider)).unwrap();
            }
            let report =
                writer::delta(&workspace.0, &workspace.db(), &[PathBuf::from(provider)]).unwrap();
            assert_eq!(
                report["reparsed_files"],
                usize::from(content.is_some()),
                "only the provider is reparsed: {report}"
            );
            for (path, bytes) in &frozen {
                assert_eq!(
                    fs::read(workspace.0.join(path)).unwrap(),
                    *bytes,
                    "unchanged cached input {path}"
                );
            }
            let conn = workspace.open();
            let edges:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id JOIN path_dictionary q ON q.path_id=d.path_id WHERE e.kind='references' AND p.path='page.vue' AND q.path='widgets/LateView.vue' AND d.kind='module'", [], |row|row.get(0)).unwrap();
            assert_eq!(edges, usize::from(enabled && content.is_some()), "cached configuration admits the canonical provider, enabled={enabled}, content={content:?}");
            drop(conn);
            complete_parity(&workspace);
        }
    }
    workspace.write(
        "atelier.config.ts",
        "export default configureAtelier({widgets:true, addons:[]});\n",
    );
    workspace.delta("atelier.config.ts");
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    complete_parity(&workspace);
}

fn configured_syntax_rules_follow_effective_rules_and_delta() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='syntax-proof'\ndependencies=['atelier']\n",
    );
    workspace.write(
        "package.json",
        r#"{"name":"syntax-proof","dependencies":{"atelier":"*"}}"#,
    );
    let registration_source = "from atelier import Service\nservice = Service('proof')\n@service.publish(label='advertised')\ndef exported(): return 1\ndef helper(): return 1\ndef ordinary(): return helper()\n";
    workspace.write("app.py", registration_source);
    workspace.write("page.html", "<button zz-read='/configured' zz-state='item' zz-values='{\"key\":1}' z-event:click='act()' z-object='{count: 1}'></button>\n");
    workspace.write("app.ts", "const entries = [{location:'/configured',handler:page}];\nfunction page() {}\nfunction client() { return ask('/configured'); }\n");
    let mut pack = serde_json::json!({
        "receivers":[], "builtins":[], "filters":[], "routes":[],
        "syntax_rules":[
            {"id":"tools","language":"python","operation":{"kind":"decorated_registration","modules":["atelier"],"constructor":"Service","member":"publish","name_keyword":"label","provider":"configured"}},
            {"id":"requests","language":"html","operation":{"kind":"html_request_attributes","request_node":"configured_request","context_node":"configured_context","namespace":"configured","context_label":"Configured context","prefixes":["zz-"],"methods":{"read":"GET"},"contexts":["state"],"values":"values"}},
            {"id":"scripts","language":"html","operation":{"kind":"html_script_attributes","event_prefixes":["z-event:"],"object_attribute":"z-object","event_scope":"event_attribute","object_scope":"object_attribute"}},
            {"id":"routes","language":"typescript","operation":{"kind":"route_object","collection":"entries","path_key":"location","handler_key":"handler","method":"GET"}},
            {"id":"client","language":"typescript","operation":{"kind":"http_client_call","callee":"ask","receivers":[],"methods":{},"default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"]}}
        ]
    });
    let manifest = ".forge/frameworks/atelier.toml";
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_active = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        for (label, sql, expected) in [
            ("decorated constructor/member/name", "SELECT count(*) FROM nodes WHERE kind='tool_registration' AND json_extract(details,'$.advertised_name')='advertised' AND json_extract(details,'$.pack')='atelier' AND json_extract(details,'$.rule')='tools'", 1),
            ("request attribute prefix/method", "SELECT count(*) FROM nodes WHERE kind='configured_request'", 1),
            ("script attribute prefixes", "SELECT count(*) FROM nodes WHERE kind='template_scope' AND (name LIKE 'event_attribute@%' OR name LIKE 'object_attribute@%')", 2),
            ("object collection/key", "SELECT count(*) FROM nodes WHERE kind='route'", 1),
            ("configured client and request", "SELECT count(*) FROM edge_occurrences WHERE kind='calls_endpoint'", 2),
        ] {
            let actual: usize = conn.query_row(sql, [], |row|row.get(0)).unwrap();
            assert_eq!(actual, if active {expected} else {0}, "{label}, active={active}");
        }
        let cached = persisted_facts(&conn);
        for (rule, expected) in [("tools", 1), ("requests", 1), ("scripts", 2), ("routes", 1)] {
            let origins = cached
                .iter()
                .flat_map(|facts| &facts.nodes)
                .filter(|node| {
                    node.details["value_flow"]["rule_origins"]
                        .as_array()
                        .is_some_and(|origins| {
                            origins
                                .iter()
                                .any(|origin| origin["pack"] == "atelier" && origin["rule"] == rule)
                        })
                })
                .count();
            assert_eq!(
                origins,
                if active { expected } else { 0 },
                "durable configured origin: {rule}"
            );
        }
        let clients=cached.iter().flat_map(|facts| &facts.references).filter(|reference| reference.kind=="calls_endpoint").filter(|reference| matches!(&reference.receiver_hint,Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack=="atelier" && origin.rule=="client")).count();
        assert_eq!(
            clients,
            usize::from(active),
            "client reference preserves actual origin"
        );
        let ordinary: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            ordinary, 1,
            "ordinary call survives syntax rule transitions"
        );
    };
    assert_active(true);
    complete_parity(&workspace);
    for suffix in [
        "def unrelated(service):\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
        "def unrelated():\n    service = object()\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
        "def unrelated():\n    service = Service('local')\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
    ] {
        workspace.write("app.py", &format!("{registration_source}\n{suffix}"));
        workspace.delta("app.py");
        assert_active(true);
        assert_eq!(workspace.open().query_row("SELECT count(*) FROM nodes WHERE kind='tool_registration'", [], |row| row.get::<_, usize>(0)).unwrap(), 1, "a different lexical receiver cannot borrow the module instance: {suffix}");
        complete_parity(&workspace);
    }
    workspace.write("app.py", registration_source);
    workspace.delta("app.py");
    for rule in pack["syntax_rules"].as_array_mut().unwrap() {
        rule["activation"] = serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    }
    write(&pack);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    for rule in pack["syntax_rules"].as_array_mut().unwrap() {
        rule["activation"] = serde_json::json!([]);
    }
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    for invalid in [
        serde_json::json!({"kind":"execute","script":"anything"}),
        serde_json::json!({"kind":"http_client_call","callee":"ask","receivers":[],"methods":{},"default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"],"extra":"unchecked"}),
    ] {
        pack["syntax_rules"][0]["operation"] = invalid;
        write(&pack);
        assert!(
            writer::build(&workspace.0, &workspace.db(), None).is_err(),
            "unknown operation/field must fail closed"
        );
    }
}

fn custom_file_rules() {
    // The same compiled production pipeline accepts an unregistered pack/API,
    // a different argument index and a different complete directory projection.
    for extension in ["toml", "yaml", "yml"] {
        let workspace = Workspace::new();
        workspace.write("pyproject.toml", "[project]\nname='custom-file-rules'\n");
        workspace.write("app/views.py", "from workshop.renderer import emit as paint\n\ndef page(request):\n    return paint('catalog/card.html', request)\n");
        const TARGET: &str = "app/views/catalog/card.html";
        workspace.write(TARGET, "<p>Custom</p>\n");
        let path = format!(".forge/frameworks/previously_unknown.{extension}");
        let mut pack = serde_json::json!({
            "receivers":[],"builtins":[],"filters":[],"routes":[],
            "semantic_rules":[{"id":"custom-file","namespace":"custom","scope":"project","activation":[],"operation":{"kind":"call_to_file","language":"python","callee":{"kind":"imported","module":"workshop.renderer","export":"emit"},"argument":0,"target":{"kind":"project_path_suffix","prefix":"views/","language":"html"},"relation":"renders_template"}}]
        });
        let write_pack = |value: &serde_json::Value| {
            let source = if extension == "toml" {
                toml::to_string(value).unwrap()
            } else {
                serde_yaml::to_string(value).unwrap()
            };
            workspace.write(&path, &source);
        };
        write_pack(&pack);
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        if extension != "toml" {
            continue;
        }
        const OVERLAP: &str = "app/views/views/views/catalog/card.html";
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/views/".into();
        workspace.write(OVERLAP, "<p>Overlapping normal segments</p>\n");
        write_pack(&pack);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(&path), PathBuf::from(OVERLAP)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((OVERLAP, 4)));
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        fs::remove_file(workspace.0.join(OVERLAP)).unwrap();
        write_pack(&pack);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(&path), PathBuf::from(OVERLAP)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        pack["semantic_rules"][0]["namespace"] = "renamed".into();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "missing/".into();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        pack["semantic_rules"][0]["activation"] =
            serde_json::json!([{"kind":"package","name":"workshop"}]);
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(
            "pyproject.toml",
            "[project]\nname='custom-file-rules'\ndependencies=['workshop']\n",
        );
        workspace.delta("pyproject.toml");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        let mut independent = pack.clone();
        independent["semantic_rules"][0]["id"] = "independent".into();
        independent["semantic_rules"][0]["namespace"] = "independent".into();
        independent["semantic_rules"][0]["operation"]["callee"]["module"] =
            "independent.renderer".into();
        workspace.write(
            ".forge/frameworks/another.toml",
            &toml::to_string(&independent).unwrap(),
        );
        workspace.write("app/other.py", "from independent.renderer import emit as paint\n\ndef page(request):\n    return paint('catalog/card.html', request)\n");
        let mut empty = pack.clone();
        empty["replace"] = true.into();
        empty["semantic_rules"] = serde_json::json!([]);
        write_pack(&empty);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from(&path),
                PathBuf::from(".forge/frameworks/another.toml"),
                PathBuf::from("app/other.py"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        file_edges(&workspace, "app/other.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(&path)).unwrap();
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        file_edges(&workspace, "app/other.py", Some((TARGET, 4)));
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        // A second namespace/rule with a different actual target makes the
        // occurrence ambiguous even though each individual rule is unique.
        let mut conflicting = pack["semantic_rules"][0].clone();
        conflicting["id"] = "conflicting-file".into();
        conflicting["namespace"] = "other".into();
        conflicting["operation"]["target"]["prefix"] = "other/".into();
        pack["semantic_rules"]
            .as_array_mut()
            .unwrap()
            .push(conflicting);
        workspace.write("app/other/catalog/card.html", "<p>Other</p>\n");
        write_pack(&pack);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from(&path),
                PathBuf::from("app/other/catalog/card.html"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        let ambiguous: String = workspace.open().query_row("SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary p ON p.path_id=rc.path_id JOIN coverage_expressions e ON e.expression_id=rc.expression_id WHERE p.path='app/views.py' AND e.expression=?1", ["paint('catalog/card.html', request)"], |row| row.get(0)).unwrap();
        assert_eq!(ambiguous, "ambiguous");
        pack["semantic_rules"].as_array_mut().unwrap().pop();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        // An actual same-name file in another Python project is not a target.
        workspace.write(
            "nested/pyproject.toml",
            "[project]\nname='nested'\ndependencies=['workshop']\n",
        );
        workspace.write("nested/views/catalog/card.html", "<p>Nested</p>\n");
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from("nested/pyproject.toml"),
                PathBuf::from("nested/views/catalog/card.html"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(TARGET)).unwrap();
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write("app/notviews/catalog/card.html", "<p>Wrong segment</p>\n");
        workspace.delta("app/notviews/catalog/card.html");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(TARGET, "<p>Restored</p>\n");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        // The canonical resolver must reject a same-spelling local module.
        workspace.write(
            "workshop/renderer.py",
            "def emit(path, request): return path\n",
        );
        workspace.delta("workshop/renderer.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
    }
}
