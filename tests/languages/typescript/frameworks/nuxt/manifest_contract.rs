use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn nuxt_vue_pinia_manifest_contract_reaches_persisted_graph() {
    use contextunity_forge_mcp::{
        core::commitments,
        db::{reader, writer},
    };
    use rusqlite::params;
    use serde_json::Value;

    let packs = [
        (
            "nuxt",
            include_str!("../../../../../src/engine/languages/manifests/nuxt.toml"),
        ),
        (
            "vue-runtime",
            include_str!("../../../../../src/engine/languages/manifests/vue-runtime.toml"),
        ),
    ];
    let cases: [(&str, &[(&str, &str)]); 4] = [
        ("pages", &[
            ("pages/index.vue", "<template><div>Home</div></template>\n"),
            ("pages/catalog/detail.vue", "<template><div>Detail</div></template>\n"),
            ("pages/catalog/index.vue", "<template><div>Catalog</div></template>\n"),
            ("pages/[id].vue", "<template><div>Dynamic</div></template>\n"),
            ("client.ts", "export function root() { return fetch('/'); }\nexport function detail() { return fetch('/catalog/detail'); }\nexport function catalog() { return fetch('/catalog'); }\nexport function dynamic() { return fetch('/[id]'); }\n"),
        ]),
        ("composables", &[
            ("client.ts", "export function request() { return useFetch('/records'); }\nexport function cached() { return useAsyncData('records', () => Promise.resolve(1)); }\n"),
            ("shadow.ts", "function useFetch() { return 1; }\nfunction useAsyncData() { return 2; }\nexport function local() { return useFetch() + useAsyncData(); }\n"),
            ("plain/package.json", "{\"dependencies\":{\"vue\":\"^3\"}}"),
            ("plain/client.ts", "useFetch('/records'); useAsyncData('records', () => Promise.resolve(1));\n"),
        ]),
        ("sfc", &[
            ("components/WidgetCard.vue", "<template><article /></template>\n"),
            ("App.vue", "<script setup lang=\"ts\">\nconst input = defineProps<{title: string}>();\nconst emit = defineEmits(['ready']);\nemit('ready');\n</script>\n<template><WidgetCard /><MissingWidget /><p>{{input.title}}</p></template>\n"),
            ("collision/package.json", "{\"dependencies\":{\"nuxt\":\"^3\"}}"),
            ("collision/nuxt.config.ts", "export default defineNuxtConfig({});\n"),
            ("collision/components/Base/WidgetCard.vue", "<template><article /></template>\n"),
            ("collision/components/BaseWidgetCard.vue", "<template><aside /></template>\n"),
            ("collision/App.vue", "<template><BaseWidgetCard /></template>\n"),
        ]),
        ("store", &[
            ("store.ts", "import {defineStore} from 'pinia';\nexport const useCart = defineStore('cart', {state: () => ({count: 0}), actions: {checkout() { return 1; }}});\nexport const unsupported = defineStore('other', dynamicOptions);\n"),
            ("client.ts", "import {useCart, unsupported} from './store';\nexport function read() { const cart = useCart(); return cart.count; }\nexport function run() { const cart = useCart(); return cart.checkout(); }\nexport function missing() { const cart = useCart(); return cart.absent(); }\nexport function dynamic() { const cart = unsupported(); return cart.checkout(); }\nexport function shadow(useCart) { const cart = useCart(); return cart.checkout(); }\n"),
            ("App.vue", "<script setup lang=\"ts\">\nimport {useCart} from './store';\nconst cart = useCart();\n</script>\n<template><p>{{cart.count}}</p><button @click=\"cart.checkout()\">Buy</button></template>\n"),
            ("duplicate.ts", "import {useCart} from './store';\nimport {useCart} from './store';\nexport function duplicate() { const cart = useCart(); return cart.checkout(); }\n"),
            ("rebound.ts", "import {useCart} from './store';\nuseCart = replacement;\nexport function rebound() { const cart = useCart(); return cart.checkout(); }\n"),
        ]),
    ];
    let renames = [
        ("nuxt", "sitekit"),
        ("defineNuxtConfig", "configureSite"),
        ("pages", "screens"),
        ("index", "home"),
        ("components", "widgets"),
        ("useFetch", "requestData"),
        ("useAsyncData", "cachedData"),
        ("defineProps", "inspectProps"),
        ("defineEmits", "publishEvents"),
        ("pinia", "basketkit"),
        ("defineStore", "makeBasket"),
        ("state", "seed"),
        ("actions", "commands"),
        ("count", "amount"),
        ("checkout", "purchase"),
    ];
    let mut failures = Vec::new();
    for (family, files) in cases {
        for renamed in [false, true] {
            let phase = format!("{family}, renamed={renamed}");
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rewrite = |input: &str| {
                    renames.iter().fold(input.to_owned(), |text, (from, to)| {
                        if renamed {
                            text.replace(from, to)
                        } else {
                            text
                        }
                    })
                };
                let workspace = Workspace::new();
                let package =
                    rewrite("{\"dependencies\":{\"nuxt\":\"^3\",\"vue\":\"^3\",\"pinia\":\"^3\"}}");
                workspace.write("package.json", &package);
                workspace.write(
                    rewrite("nuxt.config.ts"),
                    &rewrite("export default defineNuxtConfig({});\n"),
                );
                workspace.write("ordinary.ts", "export function helper() { return 1; }\n");
                workspace.write("control.ts", "import {helper} from './ordinary';\nexport function ordinary() { return helper(); }\n");
                for (path, source) in files {
                    workspace.write(rewrite(path), &rewrite(source));
                }
                let override_pack = |name: &str, source: &str, empty: bool| {
                    let text = if empty {
                        "replace = true\n".into()
                    } else {
                        let parsed: toml::Value = toml::from_str(source).unwrap();
                        let mut value = serde_json::to_value(parsed).unwrap();
                        let mut pending = vec![&mut value];
                        while let Some(value) = pending.pop() {
                            match value {
                                Value::String(text) => *text = rewrite(text),
                                Value::Array(values) => pending.extend(values),
                                Value::Object(values) => pending.extend(values.values_mut()),
                                _ => {}
                            }
                        }
                        let parsed: toml::Value = serde_json::from_value(value).unwrap();
                        format!("replace = true\n{}", toml::to_string(&parsed).unwrap())
                    };
                    workspace.write(format!(".forge/frameworks/{name}.toml"), &text);
                };
                if renamed {
                    for (name, source) in packs {
                        override_pack(name, source, false);
                    }
                }
                writer::build(&workspace.0, &workspace.db(), None).unwrap();
                let observe = |rules: bool, packages: bool, changed: bool| {
                    // This direct production reader is the admitted canonical seam witness.
                    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
                    commitments::verify(&conn).unwrap();
                    let errors: i64 = conn
                        .query_row("SELECT count(*) FROM errors", [], |row| row.get(0))
                        .unwrap();
                    assert_eq!(errors, 0, "{phase}: valid fixture");
                    let rows = |sql: &str| -> Vec<String> {
                        conn.prepare(sql)
                            .unwrap()
                            .query_map([], |row| row.get(0))
                            .unwrap()
                            .collect::<rusqlite::Result<_>>()
                            .unwrap()
                    };
                    let edge = |src: &str,
                                name: &str,
                                kind: &str,
                                dst: &str,
                                target: &str,
                                expected: bool| {
                        let source_ids: Vec<String> = conn.prepare("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name LIKE ?2 ORDER BY n.id").unwrap().query_map(params![rewrite(src), rewrite(name)], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                        let target_ids: Vec<String> = conn.prepare("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name LIKE ?2 ORDER BY n.id").unwrap().query_map(params![rewrite(dst), rewrite(target)], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                        let actual: Vec<(String, String)> = conn.prepare("SELECT DISTINCT s.id,d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path=?1 AND s.name LIKE ?2 AND e.kind=?3 AND dp.path=?4 AND d.name LIKE ?5 ORDER BY s.id,d.id").unwrap().query_map(params![rewrite(src),rewrite(name),kind,rewrite(dst),rewrite(target)], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                        if expected {
                            assert!(
                                !source_ids.is_empty(),
                                "{phase}: missing source {src}:{name}"
                            );
                            assert_eq!(
                                target_ids.len(),
                                1,
                                "{phase}: unique canonical {dst}:{target}"
                            );
                            assert!(!actual.is_empty() && actual.iter().all(|(s,d)| source_ids.contains(s) && d == &target_ids[0]), "{phase}: exact {src}:{name} --{kind}--> {dst}:{target}, got {actual:?}");
                        } else {
                            assert!(actual.is_empty(), "{phase}: forbidden {src}:{name} --{kind}--> {dst}:{target}: {actual:?}");
                        }
                    };
                    let coverage = |path: &str, expression: &str| -> Vec<(String, String)> {
                        conn.prepare("SELECT c.status,v.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.line,c.status").unwrap().query_map(params![rewrite(path),rewrite(expression)], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
                    };
                    edge(
                        "control.ts",
                        "ordinary",
                        "calls",
                        "ordinary.ts",
                        "helper",
                        true,
                    );
                    let ordinary_imports: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path='control.ts' AND dp.path='ordinary.ts' AND e.kind='imports'", [], |row| row.get(0)).unwrap();
                    assert_eq!(ordinary_imports, 1, "{phase}: ordinary import survives");
                    let active = rules && packages;
                    match family {
                        "pages" => {
                            for (function, page, route) in [
                                ("root", "pages/index.vue", "GET /"),
                                ("detail", "pages/catalog/detail.vue", "GET /catalog/detail"),
                                ("catalog", "pages/catalog/index.vue", "GET /catalog"),
                            ] {
                                edge(
                                    "client.ts",
                                    function,
                                    "calls_endpoint",
                                    page,
                                    route,
                                    active && !(changed && function == "detail"),
                                );
                                if active && !(changed && function == "detail") {
                                    edge(page, "%", "contains", page, route, true);
                                }
                            }
                            edge(
                                "client.ts",
                                "dynamic",
                                "calls_endpoint",
                                "pages/[id].vue",
                                "%",
                                false,
                            );
                            assert!(
                                coverage("client.ts", "GET /[id]")
                                    .iter()
                                    .any(|(s, _)| s == "unresolved"),
                                "{phase}: unsupported dynamic route"
                            );
                        }
                        "composables" => {
                            for expression in ["useFetch", "useAsyncData"] {
                                let observed = coverage("client.ts", expression);
                                assert_eq!(
                                    observed.iter().any(|(s, e)| s == "external"
                                        && e.contains(&rewrite("builtin:nuxt_autoimport"))),
                                    active,
                                    "{phase}: Nuxt provenance {expression}: {observed:?}"
                                );
                                assert!(
                                    coverage("plain/client.ts", expression)
                                        .iter()
                                        .all(|(_, e)| !e
                                            .contains(&rewrite("builtin:nuxt_autoimport"))),
                                    "{phase}: project isolation"
                                );
                                edge("shadow.ts", "local", "calls", "shadow.ts", expression, true);
                            }
                        }
                        "sfc" => {
                            edge(
                                "App.vue",
                                "template@%",
                                "references",
                                "App.vue",
                                "title",
                                rules,
                            );
                            edge(
                                "App.vue",
                                "%",
                                "references",
                                "components/WidgetCard.vue",
                                "WidgetCard.vue",
                                active && !changed,
                            );
                            for (path, expression, status) in [
                                ("App.vue", "MissingWidget", "unresolved"),
                                (
                                    "collision/App.vue",
                                    "BaseWidgetCard",
                                    if rules { "ambiguous" } else { "unresolved" },
                                ),
                            ] {
                                assert!(
                                    coverage(path, expression).iter().any(|(s, _)| s == status),
                                    "{phase}: {path}:{expression} {status}"
                                );
                            }
                            assert_eq!(
                                coverage("App.vue", "emit").iter().any(
                                    |(s, e)| s == "external" && e.contains("builtin:vue_macro")
                                ),
                                rules,
                                "{phase}: setup emit provenance"
                            );
                            edge(
                                "collision/App.vue",
                                "%",
                                "references",
                                "collision/components/Base/WidgetCard.vue",
                                "%",
                                false,
                            );
                            edge(
                                "collision/App.vue",
                                "%",
                                "references",
                                "collision/components/BaseWidgetCard.vue",
                                "%",
                                false,
                            );
                        }
                        "store" => {
                            for (src, name, kind, target) in [
                                ("client.ts", "read", "references", "count"),
                                ("client.ts", "run", "calls", "checkout"),
                                ("App.vue", "template@%", "references", "count"),
                                ("App.vue", "template@%", "calls", "checkout"),
                            ] {
                                edge(
                                    src,
                                    name,
                                    kind,
                                    "store.ts",
                                    target,
                                    active && !(changed && target == "count"),
                                );
                            }
                            for (path, name) in [
                                ("client.ts", "missing"),
                                ("client.ts", "dynamic"),
                                ("client.ts", "shadow"),
                                ("duplicate.ts", "duplicate"),
                                ("rebound.ts", "rebound"),
                            ] {
                                edge(path, name, "calls", "store.ts", "checkout", false);
                            }
                            assert!(
                                coverage("client.ts", "cart.absent")
                                    .iter()
                                    .any(|(s, _)| s == "unresolved"),
                                "{phase}: missing store member"
                            );
                            let mut origins = Vec::new();
                            let mut statement = conn
                                .prepare("SELECT facts_blob FROM local_facts WHERE path='store.ts'")
                                .unwrap();
                            for row in statement
                                .query_map([], |row| row.get::<_, Vec<u8>>(0))
                                .unwrap()
                            {
                                let file = reader::decode_local_facts(&row.unwrap()).unwrap();
                                let projected = crate::common::assertions::projected_facts(&file);
                                for node in projected["nodes"].as_array().into_iter().flatten() {
                                    if let Some(facts) =
                                        node["details"]["value_flow"]["factory_results"].as_array()
                                    {
                                        origins.extend(facts.iter().map(|f| {
                                            f["origin"]["pack"].as_str().unwrap().to_owned()
                                        }));
                                    }
                                }
                            }
                            assert_eq!(
                                origins.iter().any(|p| p == "vue-runtime"),
                                active,
                                "{phase}: durable Pinia origin"
                            );
                        }
                        _ => unreachable!(),
                    }
                    assert!(
                        rows("SELECT path||':'||message FROM errors ORDER BY path,line").is_empty()
                    );
                };
                observe(true, true, false);
                // Provider-only mutation must relink an unchanged consumer from persisted facts.
                let provider = match family {
                    "pages" => Some("pages/catalog/detail.vue"),
                    "sfc" => Some("components/WidgetCard.vue"),
                    "store" => Some("store.ts"),
                    _ => None,
                };
                if let Some(provider) = provider {
                    let original = files.iter().find(|(path, _)| *path == provider).unwrap().1;
                    if family == "store" {
                        workspace.write(
                            rewrite(provider),
                            &rewrite(&original.replace("count", "removedField")),
                        );
                    } else {
                        fs::remove_file(workspace.join(rewrite(provider))).unwrap();
                    }
                    workspace.delta(&rewrite(provider));
                    observe(true, true, true);
                    workspace.assert_cold_equivalent();
                    workspace.write(rewrite(provider), &rewrite(original));
                    workspace.delta(&rewrite(provider));
                    observe(true, true, false);
                    workspace.assert_cold_equivalent();
                }
                workspace.write("package.json", "{\"dependencies\":{\"vue\":\"^3\"}}");
                workspace.delta("package.json");
                observe(true, false, false);
                workspace.assert_cold_equivalent();
                workspace.write("package.json", &package);
                workspace.delta("package.json");
                observe(true, true, false);
                for (name, source) in packs {
                    override_pack(name, source, true);
                }
                writer::delta(
                    &workspace.0,
                    &workspace.db(),
                    &[
                        ".forge/frameworks/nuxt.toml".into(),
                        ".forge/frameworks/vue-runtime.toml".into(),
                    ],
                )
                .unwrap();
                observe(false, true, false);
                workspace.assert_cold_equivalent();
                for (name, source) in packs {
                    override_pack(name, source, false);
                }
                writer::delta(
                    &workspace.0,
                    &workspace.db(),
                    &[
                        ".forge/frameworks/nuxt.toml".into(),
                        ".forge/frameworks/vue-runtime.toml".into(),
                    ],
                )
                .unwrap();
                observe(true, true, false);
                workspace.assert_cold_equivalent();
                workspace.write("control.ts","import {helper} from './ordinary';\nexport function ordinary() { return helper() + 1; }\n");
                workspace.delta("control.ts");
                observe(true, true, false);
                workspace.assert_cold_equivalent();
            }));
            if let Err(error) = result {
                failures.push(format!(
                    "{phase}: {}",
                    error
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| error.downcast_ref::<&str>().map(|s| (*s).into()))
                        .unwrap_or_else(|| "panic".into())
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
