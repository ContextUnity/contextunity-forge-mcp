use super::*;

pub(super) fn case() -> Case {
    Case {
                kind: OperationKind::NamedBinding,
                vector: Some("named_bindings"),
                pack: pack(vec![], vec![
                    syntax("renderer-def", "javascript", json!({"kind":"named_binding","domain":"renderer","binding":{"kind":"call_definition","callee":{"kind":"global_member","receiver":"window.GridView","member":"registerRenderer"},"key":{"kind":"argument","index":0},"target":{"kind":"argument","index":1},"target_kind":"callable"}})),
                    syntax("renderer-ref", "python", json!({"kind":"named_binding","domain":"renderer","binding":{"kind":"call_reference","callee":{"kind":"imported","module":"tablekit","export":"Column"},"key":{"kind":"keyword","name":"renderer"}}})),
                    syntax("action-def", "javascript", json!({"kind":"named_binding","domain":"action","binding":{"kind":"call_definition","callee":{"kind":"global_member","receiver":"window.GridView","member":"registerAction"},"key":{"kind":"argument","index":0},"target":{"kind":"argument","index":1},"target_kind":"callable"}})),
                    syntax("action-ref", "python", json!({"kind":"named_binding","domain":"action","binding":{"kind":"call_reference","callee":{"kind":"imported","module":"tablekit","export":"Column"},"key":{"kind":"keyword_member","name":"extra","member":"action"}}})),
                    syntax("store-def", "javascript", json!({"kind":"named_binding","domain":"store","binding":{"kind":"call_definition","callee":{"kind":"global_member","receiver":"Alpine","member":"store"},"key":{"kind":"argument","index":0},"target":{"kind":"argument","index":1},"target_kind":"object"}})),
                    syntax("store-ref", "javascript", json!({"kind":"named_binding","domain":"store","binding":{"kind":"member_reference","root":"$store"}})),
                    syntax("event-def", "html", json!({"kind":"named_binding","domain":"event","binding":{"kind":"attribute_definition","prefix":"z-on:","scope":"event_handler"}})),
                    syntax("event-ref", "javascript", json!({"kind":"named_binding","domain":"event","binding":{"kind":"call_reference","callee":{"kind":"global","name":"$dispatch"},"key":{"kind":"argument","index":0}}})),
                    syntax("embedded", "html", json!({"kind":"html_script_attributes","event_prefixes":["z-on:"],"object_attribute":"z-data","event_scope":"event_handler","object_scope":"object_attribute"})),
                    // Observed Commerce/linked Grid call forms; configured names remain data.
                    syntax("observed-globals-js", "javascript", json!({"kind":"implicit_builtins","names":["window.Alpine.$data","window.GridView.byId.get","$fetch"]})),
                    syntax("observed-globals-ts", "typescript", json!({"kind":"implicit_builtins","names":["window.Alpine.$data","window.GridView.byId.get","$fetch"]})),
                    syntax("observed-globals-html", "html", json!({"kind":"implicit_builtins","names":["window.Alpine.$data","window.GridView.byId.get","$fetch"]})),
                    syntax("observed-globals-vue", "vue", json!({"kind":"implicit_builtins","names":["window.Alpine.$data","window.GridView.byId.get","$fetch"]})),
                ]),
                files: &[
                    ("provider.js", "export function drawA() { return 1; }\nexport function drawB() { return 2; }\nexport function openCart() { return 3; }\nexport function install() {\n    var gv = window.GridView;\n    gv.registerRenderer('stock_cart', drawA);\n    gv.registerAction('open_stock_cart', openCart);\n    gv.registerRenderer('duplicate', drawA);\n    gv.registerRenderer('duplicate', drawB);\n}\nAlpine.store('cart', {checkout() { return 1; }});\nexport function run() { return $store.cart.checkout(); }\nexport function emit() { return $dispatch('cart-open'); }\nexport function shadow(window) { window.GridView.registerRenderer('shadow', drawA); }\nexport function rebound() { var gv = window.GridView; gv = other; gv.registerRenderer('rebound', drawA); }\n"),
                    ("consumer.py", "from tablekit import Column\ndef renderer(): return Column(renderer='stock_cart')\ndef action(): return Column(extra={'action':'open_stock_cart'})\ndef missing(): return Column(renderer='other_cart')\ndef duplicate(): return Column(renderer='duplicate')\ndef shadow(): return Column(renderer='shadow')\ndef rebound(): return Column(renderer='rebound')\ndef imported(): return Column(renderer='imported')\ndef classic(): return Column(renderer='classic')\ndef module(): return Column(renderer='module')\ndef handler(): return Column(renderer='handler')\ndef uncertain(): return Column(renderer='uncertain')\ndef over_limit(): return Column(renderer='over_limit')\ndef invalid_fragment(): return Column(renderer='invalid_fragment')\n"),
                    ("classic.html", "<script>\nfunction classicA() { return 1; }\nwindow.GridView.registerRenderer('classic', classicA);\n</script>\n<script>\nfunction classicB() { return 2; }\nwindow.GridView.registerRenderer('classic', classicB);\n</script>\n"),
                    ("module.html", "<script type=\"module\">\nwindow.GridView.registerRenderer('module', function moduleDraw() { return 1; });\n</script>\n"),
                    ("handler.html", "<button z-on:install=\"window.GridView.registerRenderer('handler', function handlerDraw() { return 1; })\"></button>\n"),
                    ("uncertain.html", "<script>\nfunction uncertainDraw() { return 1; }\nwindow.GridView.registerRenderer('uncertain', uncertainDraw);\n</script>\n<script>\nwindow.GridView.registerRenderer('uncertain', chooseRenderer());\n</script>\n"),
                    ("listener.html", "<button z-on:cart-open='ordinary()'></button>\n"),
                    ("imported.js", "import window from 'sandbox';\nimport Alpine from 'sandbox';\nimport $dispatch from 'sandbox';\nimport $store from 'sandbox';\nexport function installImported() { window.GridView.registerRenderer('imported', local); Alpine.store('borrowed', {checkout() { return 2; }}); }\nexport function local() { return 1; }\nexport function importedEmit() { return $dispatch('cart-open'); }\nexport function importedRun() { return $store.cart.checkout(); }\n"),
                    ("store_consumer.js", "export function borrowed() { return $store.borrowed.checkout(); }\n"),
                    // Exact call statements from pim-dealer-edit.js:341 and actions.ts:87.
                    // The same grammar is consumed directly and through existing embedded owners.
                    ("observed.js", "const state = window.Alpine.$data(root);\nconst handle = window.GridView?.byId?.get(gridId);\n$fetch('/api');\nfunction guard(window, $fetch) { window.GridView?.byId?.get(gridId); $fetch('/api'); }\nwindow.GridView[key].get(gridId);\nwindow.GridView.byId.get(gridId).refresh();\n"),
                    ("observed.ts", "const state = window.Alpine.$data(root);\nconst handle = window.GridView?.byId?.get(gridId);\n$fetch('/api');\nfunction guard(window, $fetch) { window.GridView?.byId?.get(gridId); $fetch('/api'); }\nwindow.GridView[key].get(gridId);\nwindow.GridView.byId.get(gridId).refresh();\n"),
                    ("observed.html", "<script>\nconst state = window.Alpine.$data(root);\nconst handle = window.GridView?.byId?.get(gridId);\n$fetch('/api');\nfunction guard(window, $fetch) { window.GridView?.byId?.get(gridId); $fetch('/api'); }\nwindow.GridView[key].get(gridId);\nwindow.GridView.byId.get(gridId).refresh();\n</script>\n"),
                    ("observed.vue", "<script setup lang=\"ts\">\nconst state = window.Alpine.$data(root);\nconst handle = window.GridView?.byId?.get(gridId);\n$fetch('/api');\nfunction guard(window, $fetch) { window.GridView?.byId?.get(gridId); $fetch('/api'); }\nwindow.GridView[key].get(gridId);\nwindow.GridView.byId.get(gridId).refresh();\nAlpine.store('vue-retained', {checkout() { return 1; }});\n</script>\n<script>const retainedAfter = 1;</script>\n<template><div>Observed calls</div></template>\n"),
                ],
                edges: &[
                    ("consumer.py", "renderer", "references", "provider.js", "drawA"),
                    ("consumer.py", "action", "references", "provider.js", "openCart"),
                    ("provider.js", "run", "calls", "provider.js", "checkout"),
                    ("provider.js", "emit", "references", "listener.html", "event_handler@%"),
                    ("consumer.py", "module", "references", "module.html", "moduleDraw"),
                    ("consumer.py", "handler", "references", "handler.html", "handlerDraw"),
                ],
                negatives: &[
                    ("consumer.py", "missing", "references", "provider.js", "%"), ("consumer.py", "duplicate", "references", "provider.js", "%"),
                    ("consumer.py", "shadow", "references", "provider.js", "%"), ("consumer.py", "rebound", "references", "provider.js", "%"),
                    ("imported.js", "importedEmit", "references", "listener.html", "%"), ("imported.js", "importedRun", "calls", "provider.js", "checkout"),
                    ("consumer.py", "imported", "references", "imported.js", "%"), ("store_consumer.js", "borrowed", "calls", "imported.js", "%"),
                    ("consumer.py", "classic", "references", "classic.html", "%"), ("consumer.py", "uncertain", "references", "uncertain.html", "%"),
                    ("consumer.py", "over_limit", "references", "over_limit.html", "%"), ("consumer.py", "invalid_fragment", "references", "invalid_fragment.html", "%"),
                    ("consumer.py", "over_limit", "references", "provider.js", "drawA"), ("consumer.py", "invalid_fragment", "references", "provider.js", "drawA"),
                ],
                coverage: &[("consumer.py", 4, "%other_cart%", "unresolved"), ("consumer.py", 5, "%duplicate%", "ambiguous"),
                    ("consumer.py", 9, "%classic%", "ambiguous"), ("consumer.py", 12, "%uncertain%", "ambiguous"),
                    ("consumer.py", 13, "%over_limit%", "unresolved"), ("consumer.py", 14, "%invalid_fragment%", "unresolved"),
                    ("observed.js", 1, "window.Alpine.$data", "external"), ("observed.js", 2, "window.GridView?.byId?.get", "external"),
                    ("observed.ts", 1, "window.Alpine.$data", "external"), ("observed.ts", 2, "window.GridView?.byId?.get", "external"),
                    ("observed.html", 2, "window.Alpine.$data", "external"), ("observed.html", 3, "window.GridView?.byId?.get", "external"),
                    ("observed.vue", 2, "window.Alpine.$data", "external"), ("observed.vue", 3, "window.GridView?.byId?.get", "external"),
                    ("observed.js", 3, "$fetch", "external"), ("observed.ts", 3, "$fetch", "external"),
                    ("observed.html", 4, "$fetch", "external"), ("observed.vue", 4, "$fetch", "external"),
                    ("observed.js", 4, "window.GridView?.byId?.get", "unresolved"), ("observed.ts", 4, "window.GridView?.byId?.get", "unresolved"),
                    ("observed.html", 5, "window.GridView?.byId?.get", "unresolved"), ("observed.vue", 5, "window.GridView?.byId?.get", "unresolved"),
                    ("observed.js", 4, "$fetch", "unresolved"), ("observed.ts", 4, "$fetch", "unresolved"),
                    ("observed.html", 5, "$fetch", "unresolved"), ("observed.vue", 5, "$fetch", "unresolved"),
                    ("observed.js", 5, "window.GridView[key].get", "unresolved"), ("observed.ts", 5, "window.GridView[key].get", "unresolved"),
                    ("observed.html", 6, "window.GridView[key].get", "unresolved"), ("observed.vue", 6, "window.GridView[key].get", "unresolved"),
                    ("observed.js", 6, "window.GridView.byId.get(gridId).refresh", "unresolved"), ("observed.ts", 6, "window.GridView.byId.get(gridId).refresh", "unresolved"),
                    ("observed.html", 7, "window.GridView.byId.get(gridId).refresh", "unresolved"), ("observed.vue", 7, "window.GridView.byId.get(gridId).refresh", "unresolved")],
                consumer: "consumer.py",
                rename: &[("GridView", "TableView"), ("registerRenderer", "bindPainter"), ("registerAction", "bindAction"), ("tablekit", "cellkit"), ("Column", "Cell"), ("renderer", "painter"), ("Alpine", "Islands"), ("$store", "$registry"), ("$dispatch", "$send"), ("z-on:", "q-listen:")],
            }
}

pub(super) fn run_delta_transitions(
    workspace: &Workspace,
    case: &Case,
    phase: &str,
    manifest: &str,
    source: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    let frozen = fs::read(workspace.0.join("consumer.py")).unwrap();
    for (paths, function, line, target_path, target_name) in [
        (
            &["over_limit.html"][..],
            "over_limit",
            13,
            "provider.js",
            "drawA",
        ),
        (
            &["invalid_fragment.html", "invalid_later.html"][..],
            "invalid_fragment",
            14,
            "provider.js",
            "drawA",
        ),
        (
            &["observed.vue"][..],
            "vue_named",
            15,
            "observed.vue",
            "vueDraw",
        ),
    ] {
        let originals: Vec<_> = paths
            .iter()
            .map(|path| (*path, fs::read_to_string(workspace.0.join(path)).unwrap()))
            .collect();
        for present in [false, true] {
            for (path, original) in &originals {
                workspace.write(path, if present { original.as_str() } else { "" });
                workspace.delta(path);
            }
            let conn = workspace.open();
            let actual = edge_dst_ids(&conn, "consumer.py", function, "references");
            let resolved = if function == "vue_named" {
                present
            } else {
                !present
            };
            let expected = if !resolved {
                vec![]
            } else {
                vec![find_node_id(&conn, target_path, target_name)]
            };
            assert_eq!(
                actual, expected,
                "{phase}: invalid competitor {function} present={present}"
            );
            let states = coverage_statuses(&conn, "consumer.py", line, &format!("%{function}%"));
            assert_eq!(
                states,
                [if resolved { "resolved" } else { "unresolved" }],
                "{phase}: scoped invalid competitor coverage"
            );
            assert_eq!(
                fs::read(workspace.0.join("consumer.py")).unwrap(),
                frozen,
                "{phase}: invalid provider-only delta"
            );
            drop(conn);
            assert_delta_cold_parity(workspace);
        }
    }
    // Distinct supported origins exhaust the carrier even though every
    // rule and call is individually valid. Keep policy fixed while only
    // the provider changes, including the previously unresolved state.
    let mut overflow_pack = case.pack.clone();
    for i in 0..129 {
        overflow_pack["syntax_rules"].as_array_mut().unwrap().push(syntax(
        &format!("overflow-origin-{i}"), "javascript",
        json!({"kind":"named_binding","domain":"renderer","binding":{"kind":"call_definition","callee":{"kind":"global_member","receiver":"window.GridView","member":"registerOverflow"},"key":{"kind":"argument","index":0},"target":{"kind":"argument","index":1},"target_kind":"callable"}}),
    ));
    }
    let overflow_policy = rewrite(&toml::to_string(&overflow_pack).unwrap());
    workspace.write(
        "unrelated/pyproject.toml",
        &rewrite("[project]\nname='unrelated'\ndependencies=['tablekit']\n"),
    );
    workspace.write("unrelated/package.json", "{\"name\":\"unrelated\"}\n");
    workspace.write("unrelated/provider.js", &rewrite("export function isolatedDraw() { return 1; }\nwindow.GridView.registerRenderer('stock_cart', isolatedDraw);\n"));
    workspace.write(
        "unrelated/consumer.py",
        &rewrite(
            "from tablekit import Column\ndef unrelated(): return Column(renderer='stock_cart')\n",
        ),
    );
    let unrelated_consumer = fs::read(workspace.0.join("unrelated/consumer.py")).unwrap();
    workspace.write(manifest, &overflow_policy);
    workspace.delta(manifest);
    let vue_source = fs::read_to_string(workspace.0.join("observed.vue")).unwrap();
    let overflowing_vue = vue_source.replacen("</script>", &format!("{}\n</script>", rewrite("window.GridView.registerOverflow('stock_cart', function overflowDraw() { return 1; });")), 1);
    for overflow in [true, false, true, false] {
        workspace.write(
            "observed.vue",
            if overflow {
                &overflowing_vue
            } else {
                &vue_source
            },
        );
        workspace.delta("observed.vue");
        let conn = workspace.open();
        let actual = edge_dst_ids(&conn, "consumer.py", &rewrite("renderer"), "references");
        let expected = if overflow {
            vec![]
        } else {
            vec![find_node_id(&conn, "provider.js", "drawA")]
        };
        assert_eq!(
            actual, expected,
            "{phase}: distinct-origin provider fault overflow={overflow}"
        );
        let states = coverage_statuses(&conn, "consumer.py", 2, "%stock_cart%");
        assert_eq!(
            states,
            [if overflow { "unresolved" } else { "resolved" }],
            "{phase}: distinct-origin fault coverage"
        );
        let facts = read_facts_json(&conn, "observed.vue");
        assert_eq!(
            facts["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["details"]["named_definition_uncertainty_fault"] == true),
            overflow,
            "{phase}: valid origin exhaustion persists explicit owner fault"
        );
        assert_eq!(query_count_usize(&conn, "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'", []), 1, "{phase}: named carrier exhaustion preserves ordinary calls");
        let isolated_target = find_node_id(&conn, "unrelated/provider.js", "isolatedDraw");
        let isolated_edges =
            edge_dst_ids(&conn, "unrelated/consumer.py", "unrelated", "references");
        assert_eq!(
            isolated_edges,
            [isolated_target],
            "{phase}: nonadmitted project retains its canonical named target"
        );
        assert!(
            query_rows(&conn, "SELECT message FROM errors").is_empty(),
            "{phase}: supported origin exhaustion is not a whole-index error"
        );
        assert_eq!(
            fs::read(workspace.0.join("consumer.py")).unwrap(),
            frozen,
            "{phase}: carrier fault provider-only mutation"
        );
        assert_eq!(
            fs::read(workspace.0.join("unrelated/consumer.py")).unwrap(),
            unrelated_consumer,
            "{phase}: unrelated consumer unchanged"
        );
        drop(conn);
        assert_delta_cold_parity(workspace);
    }
    workspace.write(manifest, source);
    workspace.delta(manifest);
    let original = case
        .files
        .iter()
        .find(|(path, _)| *path == "classic.html")
        .unwrap()
        .1;
    let blocks: Vec<&str> = original.split_inclusive("</script>\n").collect();
    for (label, html, target, status) in [
        (
            "remove second",
            original.replace("window.GridView.registerRenderer('classic', classicB);", ""),
            Some("classicA"),
            "resolved",
        ),
        (
            "remove first",
            original.replace("window.GridView.registerRenderer('classic', classicA);", ""),
            Some("classicB"),
            "resolved",
        ),
        (
            "reverse islands",
            format!("{}{}", blocks[1], blocks[0]),
            None,
            "ambiguous",
        ),
        (
            "unknown alternative",
            original.replace(
                "registerRenderer('classic', classicB)",
                "registerRenderer('classic', chooseRenderer())",
            ),
            None,
            "ambiguous",
        ),
        (
            "restore alternatives",
            original.to_owned(),
            None,
            "ambiguous",
        ),
    ] {
        workspace.write("classic.html", &rewrite(&html));
        workspace.delta("classic.html");
        let conn = workspace.open();
        let actual = edge_dst_ids(&conn, "consumer.py", "classic", "references");
        let expected: Vec<String> = target
            .map(|name| find_node_id(&conn, "classic.html", name))
            .into_iter()
            .collect();
        assert_eq!(
            actual, expected,
            "{phase}: {label} retains actual classic alternatives"
        );
        let states = coverage_statuses(&conn, "consumer.py", 9, "%classic%");
        assert_eq!(states, [status], "{phase}: {label} coverage");
        assert_eq!(
            fs::read(workspace.0.join("consumer.py")).unwrap(),
            frozen,
            "classic provider-only mutation"
        );
        drop(conn);
        assert_delta_cold_parity(workspace);
    }
    for (path, caller, from, to) in [
        ("module.html", "module", "moduleDraw", "moduleOther"),
        ("handler.html", "handler", "handlerDraw", "handlerOther"),
    ] {
        let original = case.files.iter().find(|(name, _)| *name == path).unwrap().1;
        for (html, name) in [
            (original.replace(from, to), to),
            (original.to_owned(), from),
        ] {
            workspace.write(path, &rewrite(&html));
            workspace.delta(path);
            let conn = workspace.open();
            let target = find_node_id(&conn, path, name);
            let actual = edge_dst_ids(&conn, "consumer.py", caller, "references");
            assert_eq!(
                actual,
                [target],
                "{phase}: provider-only {path} uses the actual remapped Node"
            );
            assert_eq!(fs::read(workspace.0.join("consumer.py")).unwrap(), frozen);
            drop(conn);
            assert_delta_cold_parity(workspace);
        }
    }
}
