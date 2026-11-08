use super::*;

mod user_manifests;

const STARLETTE_OVERLAY_FILES: &[(&str, &str)] = &[
    ("requirements.txt", "starlette\n"),
    (
        "app.py",
        "from starlette.routing import Route\ndef records(): return []\ndef LocalRoute(path, handler): return None\nroutes=[Route('/records/', records), LocalRoute('/local/', records)]\n",
    ),
];

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

const CASES: &[Case] = &[
    Case {
        pack: "django",
        package: "django",
        family: LanguageFamily("python"),
        manifest: "pyproject.toml",
        declaration: "[project]\nname='registration'\ndependencies=['django']\n",
        empty: "[project]\nname='plain'\ndependencies=[]\n",
        files: &[("app.py", "class Record: pass\nclass View:\n    model = Record\n")],
        edge: ("app.py", "View", "uses_model", "app.py", "Record"),
        gated: true,
    },
    Case {
        pack: "starlette",
        package: "starlette",
        family: LanguageFamily("python"),
        manifest: "requirements-dev.txt",
        declaration: "starlette>=0.1\n",
        empty: "# no packages\n",
        files: &[("app.py", "from starlette.routing import Route\nfrom starlette.responses import JSONResponse\ndef records(): return []\nroutes = [Route('/records/', records)]\nclass Response(JSONResponse):\n    def known(self): return self.render({})\n")],
        edge: ("app.py", "ANY /records/", "handles", "app.py", "records"),
        gated: false,
    },
    Case {
        pack: "fastapi",
        package: "fastapi",
        family: LanguageFamily("python"),
        manifest: "setup.cfg",
        declaration: "[options]\ninstall_requires =\n    fastapi>=0.1\n",
        empty: "[options]\ninstall_requires =\n",
        files: &[("app.py", "from fastapi import FastAPI\nclass Record: pass\napp = FastAPI()\n@app.get('/records/', response_model=Record)\ndef records(): return []\n")],
        edge: ("app.py", "records", "uses_model", "app.py", "Record"),
        gated: true,
    },
    Case {
        pack: "gridviewspec",
        package: "grid-view-spec",
        family: LanguageFamily("python"),
        manifest: "Pipfile",
        declaration: "[packages]\ngrid-view-spec='*'\n",
        empty: "[packages]\n",
        files: &[("app.py", "from fastapi import FastAPI\nfrom grid_view_spec.types.table_v2 import GridViewTable, GridViewDataSource\napp = FastAPI()\n@app.get('/records/')\ndef records(): return []\ndef table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/records/'))\n")],
        edge: (
            "app.py",
            "table",
            "calls_endpoint",
            "app.py",
            "GET /records/",
        ),
        gated: false,
    },
    Case {
        pack: "nuxt",
        package: "nuxt",
        family: LanguageFamily("javascript"),
        manifest: "package.json",
        declaration: "{\"dependencies\":{\"nuxt\":\"*\"}}",
        empty: "{\"dependencies\":{}}",
        files: &[
            ("pages/index.vue", "<template><p>Home</p></template>\n"),
            (
                "client.ts",
                "export function home() { return fetch('/'); }\nexport function blocked() { return fetch('/plain'); }\n",
            ),
            ("pages/plain.vue", "<template><p>Plain</p></template>\n"),
        ],
        edge: (
            "client.ts",
            "home",
            "calls_endpoint",
            "pages/index.vue",
            "GET /",
        ),
        gated: true,
    },
    Case {
        pack: "vue-runtime",
        package: "vue",
        family: LanguageFamily("javascript"),
        manifest: "package-lock.json",
        declaration: "{\"lockfileVersion\":3,\"packages\":{\"node_modules/vue\":{\"version\":\"3\"},\"node_modules/pinia\":{\"version\":\"3\"}}}",
        empty: "{\"lockfileVersion\":3,\"packages\":{}}",
        files: &[
            (
                "store.ts",
                "import {defineStore} from 'pinia';\nexport const useCart = defineStore('cart', {state: () => ({count: 0}), actions: {checkout() { return 1; }}});\n",
            ),
            (
                "client.ts",
                "import {useCart} from './store';\nexport function run() { const cart = useCart(); return cart.checkout(); }\n",
            ),
        ],
        edge: ("client.ts", "run", "calls", "store.ts", "checkout"),
        gated: true,
    },
    Case {
        pack: "htmx",
        package: "htmx.org",
        family: LanguageFamily("javascript"),
        manifest: "pnpm-lock.yaml",
        declaration: "lockfileVersion: '9.0'\npackages:\n  'htmx.org@2.0.0':\n    resolution: {integrity: example}\n",
        empty: "lockfileVersion: '9.0'\npackages: {}\n",
        files: &[
            (
                "backend.py",
                "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/records/')\ndef records(): return []\n",
            ),
            (
                "index.html",
                "<button hx-get='/records/' hx-target='#records' hx-swap='outerHTML'></button>\n",
            ),
        ],
        edge: (
            "index.html",
            "GET /records/",
            "calls_endpoint",
            "backend.py",
            "GET /records/",
        ),
        gated: false,
    },
    Case {
        pack: "alpinejs",
        package: "alpinejs",
        family: LanguageFamily("javascript"),
        manifest: "yarn.lock",
        declaration: "alpinejs@^3.0.0:\n  version \"3.0.0\"\n",
        empty: "# no packages\n",
        files: &[("app.js", "Alpine.store('cart', {checkout() { return 1; }});\nexport function run() { return $store.cart.checkout(); }\n")],
        edge: ("app.js", "run", "calls", "app.js", "checkout"),
        gated: false,
    },
];

fn sources_for(
    case: &Case,
    declaration: Option<&str>,
    local_manifest: Option<(&str, &str)>,
    nested_scope: bool,
) -> VirtualWorkspace {
    let mut files = BTreeMap::new();
    if let Some(declaration) = declaration {
        files.insert(case.manifest.to_owned(), declaration.to_owned());
    }
    if nested_scope {
        files.insert(format!("plain/{}", case.manifest), case.empty.to_owned());
    }
    files.insert(
        "providers.py".to_owned(),
        "def provide(): return 1\n".to_owned(),
    );
    files.insert(
        "ordinary.py".to_owned(),
        "from providers import provide\ndef ordinary(): return provide()\n".to_owned(),
    );
    for &(path, source) in case.files {
        if nested_scope && case.pack == "nuxt" && path == "pages/plain.vue" {
            continue;
        }
        files.insert(path.to_owned(), source.to_owned());
        if nested_scope && case.gated && case.pack != "nuxt" {
            files.insert(format!("plain/{path}"), source.to_owned());
        }
    }
    if nested_scope && case.pack == "nuxt" {
        files.insert(
            "pages/plain/package.json".to_owned(),
            "{\"dependencies\":{}}".to_owned(),
        );
        files.insert(
            "pages/plain/index.vue".to_owned(),
            "<template><p>Nested</p></template>\n".to_owned(),
        );
    }
    if let Some((path, source)) = local_manifest {
        files.insert(path.to_owned(), source.to_owned());
    }
    VirtualWorkspace::from_map(files)
}

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
#[test]
fn bundled_framework_packs_activate_only_in_their_workspace() {
    let cases = CASES;

    let edge = |conn: &Connection, case: &Case, prefix: &str, active: bool| {
        let (source_path, source, kind, target_path, target) = case.edge;
        let source_path = format!("{prefix}{source_path}");
        let target_path = format!("{prefix}{target_path}");
        let actual = conn
            .prepare_cached(
                "SELECT s.id, d.id FROM edge_occurrences e \
                     JOIN nodes s ON s.node_hash=e.src_hash \
                     JOIN nodes d ON d.node_hash=e.dst_hash \
                     JOIN path_dictionary sp ON sp.path_id=s.path_id \
                     JOIN path_dictionary dp ON dp.path_id=d.path_id \
                     WHERE sp.path=?1 AND s.name=?2 AND e.kind=?3 AND dp.path=?4 AND d.name=?5 \
                     ORDER BY s.id, d.id",
            )
            .unwrap()
            .query_map(
                params![&source_path, source, kind, &target_path, target],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<(String, String)>>();
        if active {
            assert_eq!(
                actual.len(),
                1,
                "{}: persisted canonical edge: {actual:?}",
                case.pack
            );
            for (path, name, id) in [
                (source_path.as_str(), source, &actual[0].0),
                (target_path.as_str(), target, &actual[0].1),
            ] {
                assert_eq!(
                    named_node_ids(conn, path, name).as_slice(),
                    std::slice::from_ref(id),
                    "unique canonical declaration"
                );
            }
        } else {
            assert!(actual.is_empty(), "{} noneligible {actual:?}", case.pack);
        }
    };
    let observe = |conn: &Connection, case: &Case, active: bool| {
        commitments::verify(conn).unwrap();
        assert!(error_messages(conn).is_empty());
        edge(conn, case, "", active);
        assert_eq!(
            call_target_names(conn, Some("ordinary.py"), "ordinary"),
            ["provide"]
        );
        assert_eq!(coverage_status(conn, "ordinary.py", "provide"), "resolved");
    };
    let inventory_registry = FrameworkHarness::from_manifests(&[("requirements.txt", "django\n")])
        .unwrap()
        .registry;
    let inventory_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/forge-index/src/languages/manifests");
    let inventory = std::fs::read_dir(inventory_directory)
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
    let mut identities = std::collections::BTreeSet::new();
    for path in inventory {
        let id = path.file_stem().unwrap().to_str().unwrap();
        assert!(
            identities.insert(id.to_owned()),
            "duplicate source inventory identity {id}"
        );
        assert!(
            inventory_registry
                .framework_manifest_for_path(LanguageFamily("python"), "inventory.py", id, "django")
                .is_some(),
            "supported source file {} absent from production bundle inventory",
            path.display()
        );
    }

    for case in cases {
        let sources = sources_for(case, Some(case.declaration), None, true);
        let indexed = sources.index().unwrap();
        assert!(
            indexed
                .registry
                .framework_manifest_for_path(case.family, case.edge.0, case.pack, case.package)
                .is_some(),
            "{}: declaration admits the bundled pack",
            case.pack
        );
        assert!(
            indexed
                .registry
                .framework_manifest_for_path(case.family, "plain/probe.py", case.pack, case.package)
                .is_none(),
            "nearest empty package scope"
        );
        observe(&indexed.conn, case, true);
        if case.gated && case.pack != "nuxt" {
            edge(&indexed.conn, case, "plain/", false);
        }
        if case.pack == "nuxt" {
            assert!(
                !route_names(&indexed.conn)
                    .iter()
                    .any(|name| name == "GET /plain"),
                "nearest empty package scope suppresses nested Nuxt route"
            );
        }

        let unrelated_sources = sources_for(case, None, None, false);
        let cached_registry = FrameworkHarness::bundled_registry();
        assert!(
            cached_registry
                .framework_manifest_for_path(case.family, case.edge.0, case.pack, case.package)
                .is_none(),
            "unrelated workspace declaration isolation"
        );
        let unrelated_conn = unrelated_sources
            .index_with_registry(cached_registry)
            .unwrap();
        observe(&unrelated_conn, case, !case.gated);

        if case.pack == "django" {
            let before = indexed.registry.digest().to_owned();
            let mut pack: toml::Value = toml::from_str("enabled = true").unwrap();
            pack.as_table_mut()
                .unwrap()
                .insert("enabled".into(), false.into());
            let disabled_source = toml::to_string(&pack).unwrap();
            let disabled: languages::manifests::FrameworkManifest =
                toml::from_str(&disabled_source).unwrap();
            assert_eq!(
                serde_json::to_value(disabled).unwrap()["enabled"],
                false,
                "false survives serialization"
            );
            let manifest = ".forge/frameworks/django.toml";
            let disabled_index = sources_for(
                case,
                Some(case.declaration),
                Some((manifest, &disabled_source)),
                true,
            )
            .index()
            .unwrap();
            assert_ne!(disabled_index.registry.digest(), before);
            assert!(
                disabled_index
                    .registry
                    .framework_manifest_for_path(case.family, case.edge.0, case.pack, case.package)
                    .is_none(),
                "disabled local tombstone must not fall back to bundled pack"
            );
            observe(&disabled_index.conn, case, false);

            pack["enabled"] = true.into();
            let enabled_source = toml::to_string(&pack).unwrap();
            let enabled_index = sources_for(
                case,
                Some(case.declaration),
                Some((manifest, &enabled_source)),
                true,
            )
            .index()
            .unwrap();
            observe(&enabled_index.conn, case, true);
            let restored_defaults = sources_for(case, Some(case.declaration), None, true)
                .index()
                .unwrap();
            observe(&restored_defaults.conn, case, true);
        }
    }

    for (extension, mode) in [("toml", ""), ("yaml", "extend"), ("yml", "replace")] {
        let mut value = serde_json::json!({
            "route_calls": [{"callee":"LocalRoute","method":"GET","path_argument":0,"handler_argument":1}]
        });
        if !mode.is_empty() {
            value[mode] = true.into();
        }
        let serialize = |value: &serde_json::Value| {
            if extension == "toml" {
                let mut out = String::new();
                if value.get("extend").and_then(|item| item.as_bool()) == Some(true) {
                    out.push_str("extend = true\n");
                }
                if value.get("replace").and_then(|item| item.as_bool()) == Some(true) {
                    out.push_str("replace = true\n");
                }
                if value.get("enabled").and_then(|item| item.as_bool()) == Some(false) {
                    out.push_str("enabled = false\n");
                }
                out.push_str(
                    "[[route_calls]]\ncallee = \"LocalRoute\"\nmethod = \"GET\"\npath_argument = 0\nhandler_argument = 1\n",
                );
                out
            } else {
                serde_yaml::to_string(value).unwrap()
            }
        };
        let manifest = format!(".forge/frameworks/starlette.{extension}");
        let index_with = |local_manifest: Option<&str>| {
            let mut sources = VirtualWorkspace::from_files(STARLETTE_OVERLAY_FILES);
            if let Some(source) = local_manifest {
                sources.write(&manifest, source);
            }
            sources.index().unwrap()
        };
        let check = |registry: &DependencyRegistry,
                     conn: &Connection,
                     enabled: bool,
                     bundled: bool,
                     overlay: bool| {
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
            commitments::verify(conn).unwrap();
            let mut expected = Vec::new();
            if enabled && bundled {
                expected.push("ANY /records/->records".to_owned());
            }
            if enabled && overlay {
                expected.push("GET /local/->records".to_owned());
            }
            assert_eq!(
                route_handle_labels(conn),
                expected,
                "{extension} {mode}, routes={:?}",
                route_names(conn)
            );
            assert_eq!(
                query_rows_params(
                    conn,
                    "SELECT d.name FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND d.name=?1",
                    ["LocalRoute"],
                ),
                ["LocalRoute"],
                "ordinary call survives policy changes"
            );
        };

        let enabled_source = serialize(&value);
        let enabled = index_with(Some(&enabled_source));
        check(
            &enabled.registry,
            &enabled.conn,
            true,
            mode != "replace",
            true,
        );

        value["enabled"] = false.into();
        let disabled_source = serialize(&value);
        let disabled = index_with(Some(&disabled_source));
        check(&disabled.registry, &disabled.conn, false, false, false);
        assert!(
            !disabled
                .registry
                .framework_rule_diagnostics([("app.py", "python")])
                .is_empty(),
            "disabled pack retains removed eligible identity diagnostics"
        );

        value["enabled"] = true.into();
        let reenabled_source = serialize(&value);
        let reenabled = index_with(Some(&reenabled_source));
        check(
            &reenabled.registry,
            &reenabled.conn,
            true,
            mode != "replace",
            true,
        );

        let restored_defaults = index_with(None);
        check(
            &restored_defaults.registry,
            &restored_defaults.conn,
            true,
            true,
            false,
        );
        assert_eq!(route_names(&restored_defaults.conn), ["ANY /records/"]);

        let atelier_manifest = format!(".forge/frameworks/atelier.{extension}");
        let mut atelier_sources = VirtualWorkspace::from_files(STARLETTE_OVERLAY_FILES);
        atelier_sources.write(&atelier_manifest, &reenabled_source);
        let atelier = atelier_sources.index().unwrap();
        assert_eq!(
            atelier
                .registry
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
        assert_eq!(
            route_handle_labels(&atelier.conn),
            ["ANY /records/->records", "GET /local/->records"],
            "{extension} atelier pack adds LocalRoute without disabling ungated starlette"
        );
    }
}
