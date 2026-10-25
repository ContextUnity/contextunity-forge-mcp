use super::*;

mod user_manifests;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
#[test]
fn bundled_framework_packs_activate_only_in_their_workspace() {
    struct Case {
        pack: &'static str,
        package: &'static str,
        family: LanguageFamily,
        manifest: &'static str,
        declaration: &'static str,
        empty: &'static str,
        files: &'static [(&'static str, &'static str)],
        edge: (
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            &'static str,
        ),
        gated: bool,
    }
    let cases = [
        Case { pack: "django", package: "django", family: LanguageFamily("python"), manifest: "pyproject.toml", declaration: "[project]\nname='registration'\ndependencies=['django']\n", empty: "[project]\nname='plain'\ndependencies=[]\n", files: &[("app.py", "class Record: pass\nclass View:\n    model = Record\n")], edge: ("app.py","View","uses_model","app.py","Record"), gated: true },
        Case { pack: "starlette", package: "starlette", family: LanguageFamily("python"), manifest: "requirements-dev.txt", declaration: "starlette>=0.1\n", empty: "# no packages\n", files: &[("app.py", "from starlette.routing import Route\nfrom starlette.responses import JSONResponse\ndef records(): return []\nroutes = [Route('/records/', records)]\nclass Response(JSONResponse):\n    def known(self): return self.render({})\n")], edge: ("app.py","ANY /records/","handles","app.py","records"), gated: false },
        Case { pack: "fastapi", package: "fastapi", family: LanguageFamily("python"), manifest: "setup.cfg", declaration: "[options]\ninstall_requires =\n    fastapi>=0.1\n", empty: "[options]\ninstall_requires =\n", files: &[("app.py", "from fastapi import FastAPI\nclass Record: pass\napp = FastAPI()\n@app.get('/records/', response_model=Record)\ndef records(): return []\n")], edge: ("app.py","records","uses_model","app.py","Record"), gated: true },
        Case { pack: "gridviewspec", package: "grid-view-spec", family: LanguageFamily("python"), manifest: "Pipfile", declaration: "[packages]\ngrid-view-spec='*'\n", empty: "[packages]\n", files: &[("app.py", "from fastapi import FastAPI\nfrom grid_view_spec.types.table_v2 import GridViewTable, GridViewDataSource\napp = FastAPI()\n@app.get('/records/')\ndef records(): return []\ndef table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/records/'))\n")], edge: ("app.py","table","calls_endpoint","app.py","GET /records/"), gated: false },
        Case { pack: "nuxt", package: "nuxt", family: LanguageFamily("javascript"), manifest: "package.json", declaration: "{\"dependencies\":{\"nuxt\":\"*\"}}", empty: "{\"dependencies\":{}}", files: &[("pages/index.vue","<template><p>Home</p></template>\n"),("client.ts","export function home() { return fetch('/'); }\nexport function blocked() { return fetch('/plain'); }\n"),("pages/plain.vue","<template><p>Plain</p></template>\n")], edge: ("client.ts","home","calls_endpoint","pages/index.vue","GET /"), gated: true },
        Case { pack: "vue-runtime", package: "vue", family: LanguageFamily("javascript"), manifest: "package-lock.json", declaration: "{\"lockfileVersion\":3,\"packages\":{\"node_modules/vue\":{\"version\":\"3\"},\"node_modules/pinia\":{\"version\":\"3\"}}}", empty: "{\"lockfileVersion\":3,\"packages\":{}}", files: &[("store.ts","import {defineStore} from 'pinia';\nexport const useCart = defineStore('cart', {state: () => ({count: 0}), actions: {checkout() { return 1; }}});\n"),("client.ts","import {useCart} from './store';\nexport function run() { const cart = useCart(); return cart.checkout(); }\n")], edge: ("client.ts","run","calls","store.ts","checkout"), gated: true },
        Case { pack: "htmx", package: "htmx.org", family: LanguageFamily("javascript"), manifest: "pnpm-lock.yaml", declaration: "lockfileVersion: '9.0'\npackages:\n  'htmx.org@2.0.0':\n    resolution: {integrity: example}\n", empty: "lockfileVersion: '9.0'\npackages: {}\n", files: &[("backend.py","from fastapi import FastAPI\napp = FastAPI()\n@app.get('/records/')\ndef records(): return []\n"),("index.html","<button hx-get='/records/' hx-target='#records' hx-swap='outerHTML'></button>\n")], edge: ("index.html","GET /records/","calls_endpoint","backend.py","GET /records/"), gated: false },
        Case { pack: "alpinejs", package: "alpinejs", family: LanguageFamily("javascript"), manifest: "yarn.lock", declaration: "alpinejs@^3.0.0:\n  version \"3.0.0\"\n", empty: "# no packages\n", files: &[("app.js","Alpine.store('cart', {checkout() { return 1; }});\nexport function run() { return $store.cart.checkout(); }\n")], edge: ("app.js","run","calls","app.js","checkout"), gated: false },
    ];
    let edge = |conn: &Connection, case: &Case, prefix: &str, active: bool| {
        let (source_path, source, kind, target_path, target) = case.edge;
        assert_edge_canonical(
            conn,
            &format!("{prefix}{source_path}"),
            source,
            kind,
            &format!("{prefix}{target_path}"),
            target,
            active,
            case.pack,
        );
    };
    let observe = |workspace: &Workspace, case: &Case, active: bool| {
        let conn = workspace.open();
        commitments::verify(&conn).unwrap();
        assert!(query_rows(&conn, "SELECT message FROM errors").is_empty());
        edge(&conn, case, "", active);
        assert_eq!(query_rows(&conn,"SELECT d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='ordinary.py' AND s.name='ordinary' AND e.kind='calls'"),["provide"]);
        assert_eq!(coverage_status(&conn, "ordinary.py", "provide"), "resolved");
        drop(conn);
        workspace.assert_cold_equivalent();
    };
    let mut failures = Vec::new();
    let inventory_workspace = Workspace::new();
    inventory_workspace.write("requirements.txt", "django\n");
    let inventory_registry =
        DependencyRegistry::collect_with_scan_config(&inventory_workspace.0, None);
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/languages/manifests");
    let mut inventory = fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("toml" | "yaml" | "yml")
            )
        })
        .collect::<Vec<_>>();
    inventory.sort();
    let mut identities = std::collections::BTreeSet::new();
    for path in inventory {
        let id = path.file_stem().unwrap().to_str().unwrap();
        assert!(
            identities.insert(id.to_owned()),
            "duplicate source inventory identity {id}"
        );
        if inventory_registry
            .framework_manifest_for_path(LanguageFamily("python"), "inventory.py", id, "django")
            .is_none()
        {
            failures.push(format!(
                "supported source file {} absent from production bundle inventory",
                path.display()
            ));
        }
    }
    for case in &cases {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let workspace = Workspace::new();
            workspace.write(case.manifest, case.declaration);
            workspace.write(format!("plain/{}", case.manifest), case.empty);
            workspace.write("providers.py", "def provide(): return 1\n");
            workspace.write(
                "ordinary.py",
                "from providers import provide\ndef ordinary(): return provide()\n",
            );
            for (path, source) in case.files {
                workspace.write(path, source);
                if case.gated && case.pack != "nuxt" {
                    workspace.write(format!("plain/{path}"), source);
                }
            }
            if case.pack == "nuxt" {
                workspace.write("pages/plain/package.json", "{\"dependencies\":{}}");
                workspace.write(
                    "pages/plain/index.vue",
                    "<template><p>Nested</p></template>\n",
                );
                fs::remove_file(workspace.0.join("pages/plain.vue")).unwrap();
            }
            let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
            let loaded = registry.framework_manifest_for_path(
                case.family,
                case.edge.0,
                case.pack,
                case.package,
            );
            if loaded.is_none() {
                eprintln!("{}: standard loader missing filename identity", case.pack);
            }
            assert!(
                registry
                    .framework_manifest_for_path(
                        case.family,
                        "plain/probe.py",
                        case.pack,
                        case.package
                    )
                    .is_none(),
                "nearest empty package scope"
            );
            writer::build(&workspace.0, &workspace.db(), None).unwrap();
            observe(&workspace, case, true);
            assert_eq!(loaded.unwrap().name, case.pack);
            if case.gated && case.pack != "nuxt" {
                edge(&workspace.open(), case, "plain/", false);
            }
            if case.pack == "nuxt" {
                assert!(
                    query_rows(
                        &workspace.open(),
                        "SELECT name FROM nodes WHERE kind='route' AND name='GET /plain'"
                    )
                    .is_empty(),
                    "nearest empty package scope suppresses nested Nuxt route"
                );
            }
            let unrelated = Workspace::new();
            unrelated.write("providers.py", "def provide(): return 1\n");
            unrelated.write(
                "ordinary.py",
                "from providers import provide\ndef ordinary(): return provide()\n",
            );
            write_files(&unrelated, case.files);
            assert!(
                DependencyRegistry::collect_with_scan_config(&unrelated.0, None)
                    .framework_manifest_for_path(case.family, case.edge.0, case.pack, case.package)
                    .is_none(),
                "unrelated workspace declaration isolation"
            );
            writer::build(&unrelated.0, &unrelated.db(), None).unwrap();
            observe(&unrelated, case, !case.gated);
            fs::remove_file(workspace.0.join(case.manifest)).unwrap();
            workspace.delta(case.manifest);
            observe(&workspace, case, !case.gated);
            workspace.write(case.manifest, case.declaration);
            workspace.delta(case.manifest);
            observe(&workspace, case, true);
        }));
        if let Err(error) = result {
            let message = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|value| (*value).into()))
                .unwrap_or_default();
            failures.push(format!("{} / {}: {message}", case.pack, case.manifest));
        }
    }
    assert!(
        failures.is_empty(),
        "standard bundle persisted registration:\n{}",
        failures.join("\n")
    );

    for case in &cases {
        let workspace = Workspace::new();
        workspace.write(case.manifest, case.declaration);
        workspace.write("providers.py", "def provide(): return 1\n");
        workspace.write(
            "ordinary.py",
            "from providers import provide\ndef ordinary(): return provide()\n",
        );
        write_files(&workspace, case.files);
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        observe(&workspace, case, true);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("src/engine/languages/manifests/{}.toml", case.pack));
        let mut pack: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        pack.as_table_mut()
            .unwrap()
            .insert("enabled".into(), false.into());
        let disabled = toml::to_string(&pack).unwrap();
        let parsed: languages::manifests::FrameworkManifest = toml::from_str(&disabled).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["enabled"],
            false,
            "false survives serialization"
        );
        let manifest = format!(".forge/frameworks/{}.toml", case.pack);
        let before = DependencyRegistry::collect_with_scan_config(&workspace.0, None)
            .digest()
            .to_owned();
        workspace.write(&manifest, &disabled);
        workspace.delta(&manifest);
        let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
        assert_ne!(registry.digest(), before, "enabled input is digest-bound");
        assert!(
            registry
                .framework_manifest_for_path(case.family, case.edge.0, case.pack, case.package)
                .is_none(),
            "disabled local tombstone must not fall back to bundled pack"
        );
        observe(&workspace, case, false);
        pack["enabled"] = true.into();
        workspace.write(&manifest, &toml::to_string(&pack).unwrap());
        workspace.delta(&manifest);
        observe(&workspace, case, true);
        fs::remove_file(workspace.0.join(&manifest)).unwrap();
        workspace.delta(&manifest);
        observe(&workspace, case, true);
    }

    for (extension, mode) in [("toml", ""), ("yaml", "extend"), ("yml", "replace")] {
        let workspace = Workspace::new();
        workspace.write("requirements.txt", "starlette\n");
        workspace.write("app.py","from starlette.routing import Route\ndef records(): return []\ndef LocalRoute(path,handler): return None\nroutes=[Route('/records/',records),LocalRoute('/local/',records)]\n");
        let value = serde_json::json!({"route_calls":[{"callee":"LocalRoute","method":"GET","path_argument":0,"handler_argument":1}]});
        let mut value = value;
        if !mode.is_empty() {
            value[mode] = true.into();
        }
        let serialize = |value: &serde_json::Value| {
            if extension == "toml" {
                toml::to_string(&toml::Value::try_from(value).unwrap()).unwrap()
            } else {
                serde_yaml::to_string(value).unwrap()
            }
        };
        let manifest = format!(".forge/frameworks/starlette.{extension}");
        workspace.write(&manifest, &serialize(&value));
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        let check = |enabled: bool, bundled: bool| {
            let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
            let loaded = registry.framework_manifest_for_path(
                LanguageFamily("python"),
                "app.py",
                "starlette",
                "starlette",
            );
            assert_eq!(
                loaded.is_some(),
                enabled,
                "{extension} effective pack admission"
            );
            let conn = workspace.open();
            commitments::verify(&conn).unwrap();
            let routes = query_rows(&conn, "SELECT n.name||'->'||d.name FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='handles' ORDER BY n.name");
            let mut expected = Vec::new();
            if enabled && bundled {
                expected.push("ANY /records/->records".to_owned());
            }
            if enabled {
                expected.push("GET /local/->records".to_owned());
            }
            assert_eq!(routes, expected, "{extension} {mode}");
            assert_eq!(query_rows(&conn, "SELECT d.name FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND d.name='LocalRoute'"), ["LocalRoute"], "ordinary call survives policy changes");
            drop(conn);
            workspace.assert_cold_equivalent();
        };
        check(true, mode != "replace");
        value["enabled"] = false.into();
        workspace.write(&manifest, &serialize(&value));
        workspace.delta(&manifest);
        check(false, false);
        let disabled = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
        assert!(
            !disabled
                .framework_rule_diagnostics([("app.py", "python")])
                .is_empty(),
            "disabled pack retains removed eligible identity diagnostics"
        );
        value["enabled"] = true.into();
        workspace.write(&manifest, &serialize(&value));
        workspace.delta(&manifest);
        check(true, mode != "replace");
        fs::remove_file(workspace.0.join(&manifest)).unwrap();
        workspace.delta(&manifest);
        assert_eq!(
            query_rows(
                &workspace.open(),
                "SELECT n.name FROM nodes n WHERE n.kind='route' ORDER BY n.name"
            ),
            ["ANY /records/"],
            "deletion restores bundled defaults"
        );
        workspace.assert_cold_equivalent();
        let manifest = format!(".forge/frameworks/atelier.{extension}");
        workspace.write(&manifest, &serialize(&value));
        workspace.delta(&manifest);
        assert_eq!(
            DependencyRegistry::collect_with_scan_config(&workspace.0, None)
                .framework_manifest_for_path(
                    LanguageFamily("python"),
                    "app.py",
                    "atelier",
                    "not-installed"
                )
                .unwrap()
                .name,
            "atelier"
        );
        workspace.assert_cold_equivalent();
    }
}
