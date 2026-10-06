use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::{ast, languages, scanner},
};
use std::{collections::BTreeSet, fs, path::PathBuf, time::SystemTime};

struct Fixture {
    id: &'static str,
    feature: &'static str,
    enabled: bool,
    path: &'static str,
    source: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        id: "html",
        feature: "lang-html",
        enabled: cfg!(feature = "lang-html"),
        path: "sample.html",
        source: "<html><body><p>marker</p></body></html>\n",
    },
    Fixture {
        id: "yaml",
        feature: "lang-yaml",
        enabled: cfg!(feature = "lang-yaml"),
        path: "sample.yaml",
        source: "marker: true\n",
    },
    Fixture {
        id: "toml",
        feature: "lang-toml",
        enabled: cfg!(feature = "lang-toml"),
        path: "sample.toml",
        source: "marker = true\n",
    },
    Fixture {
        id: "python",
        feature: "lang-python",
        enabled: cfg!(feature = "lang-python"),
        path: "sample.py",
        source: "def marker(): pass\n",
    },
    Fixture {
        id: "rust",
        feature: "lang-rust",
        enabled: cfg!(feature = "lang-rust"),
        path: "sample.rs",
        source: "pub fn marker() {}\n",
    },
    Fixture {
        id: "typescript",
        feature: "lang-typescript",
        enabled: cfg!(feature = "lang-typescript"),
        path: "sample.ts",
        source: "export function marker() {}\n",
    },
    Fixture {
        id: "javascript",
        feature: "lang-typescript",
        enabled: cfg!(feature = "lang-typescript"),
        path: "sample.js",
        source: "export function marker() {}\n",
    },
    Fixture {
        id: "vue",
        feature: "lang-vue",
        enabled: cfg!(feature = "lang-vue"),
        path: "sample.vue",
        source: "<script>function marker() {}</script>\n",
    },
    Fixture {
        id: "proto",
        feature: "lang-proto",
        enabled: cfg!(feature = "lang-proto"),
        path: "sample.proto",
        source: "syntax = \"proto3\";\nmessage marker {}\n",
    },
    Fixture {
        id: "go",
        feature: "lang-go",
        enabled: cfg!(feature = "lang-go"),
        path: "sample.go",
        source: "package sample\nfunc marker() {}\n",
    },
    Fixture {
        id: "java",
        feature: "lang-java",
        enabled: cfg!(feature = "lang-java"),
        path: "Sample.java",
        source: "class Sample { void marker() {} }\n",
    },
    Fixture {
        id: "csharp",
        feature: "lang-csharp",
        enabled: cfg!(feature = "lang-csharp"),
        path: "Sample.cs",
        source: "class Sample { void marker() {} }\n",
    },
    Fixture {
        id: "kotlin",
        feature: "lang-kotlin",
        enabled: cfg!(feature = "lang-kotlin"),
        path: "sample.kt",
        source: "fun marker() {}\n",
    },
    Fixture {
        id: "php",
        feature: "lang-php",
        enabled: cfg!(feature = "lang-php"),
        path: "sample.php",
        source: "<?php function marker() {}\n",
    },
    Fixture {
        id: "ruby",
        feature: "lang-ruby",
        enabled: cfg!(feature = "lang-ruby"),
        path: "sample.rb",
        source: "def marker; 1; end\n",
    },
    Fixture {
        id: "c",
        feature: "lang-c",
        enabled: cfg!(feature = "lang-c"),
        path: "sample.c",
        source: "int marker(void) { return 1; }\n",
    },
    Fixture {
        id: "cpp",
        feature: "lang-cpp",
        enabled: cfg!(feature = "lang-cpp"),
        path: "sample.cpp",
        source: "int marker() { return 1; }\n",
    },
];

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_feature_contract_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registry_and_extraction_match_the_compiled_feature_contract() {
    let expected: BTreeSet<_> = FIXTURES
        .iter()
        .filter(|fixture| fixture.enabled)
        .map(|fixture| fixture.id)
        .collect();
    assert_eq!(
        languages::profiles()
            .map(|p| p.id())
            .collect::<BTreeSet<_>>(),
        expected
    );
    let features: BTreeSet<_> = FIXTURES
        .iter()
        .filter(|fixture| fixture.enabled)
        .map(|fixture| fixture.feature)
        .collect();
    assert_eq!(
        languages::ENABLED_LANGUAGE_FEATURES
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        features
    );
    for fixture in FIXTURES {
        assert_eq!(
            scanner::language(fixture.path.as_ref()),
            fixture.enabled.then_some((fixture.id, false)),
            "{}",
            fixture.id
        );
        let facts = ast::extract(fixture.path, fixture.id, fixture.source);
        if fixture.enabled {
            let facts = facts.unwrap();
            assert!(
                facts.errors.is_empty(),
                "{}: {:?}",
                fixture.id,
                facts.errors
            );
            assert!(
                facts.nodes.iter().any(|node| {
                    if fixture.id == "html" {
                        node.id == "module:sample.html" && node.kind == "module"
                    } else {
                        node.name == "marker"
                    }
                }),
                "{}",
                fixture.id
            );
        } else {
            let error = facts.unwrap_err().to_string();
            assert!(error.contains("disabled"), "{error}");
            assert!(error.contains(fixture.feature), "{error}");
            assert!(error.contains("compiled languages:"), "{error}");
            let error = ast::parser(fixture.id, fixture.path)
                .err()
                .expect("disabled parser")
                .to_string();
            assert!(error.contains(fixture.feature), "{error}");
        }
    }
    let unknown = ast::extract("sample.unknown", "unknown", "")
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("unknown AST language"), "{unknown}");
    if expected.is_empty() {
        assert!(unknown.contains("document-only build"), "{unknown}");
    }
}

#[test]
fn scanner_and_database_keep_documents_and_only_compiled_languages() {
    let workspace = Workspace::new();
    for fixture in FIXTURES {
        fs::write(workspace.0.join(fixture.path), fixture.source).unwrap();
    }
    fs::write(
        workspace.0.join("README.md"),
        "# Feature fixture\n\nDocuments remain available.\n",
    )
    .unwrap();
    let db = workspace.0.join(".forge/code-map.sqlite");
    writer::build(&workspace.0, &db, None).unwrap();
    let conn = reader::open(&db, &workspace.0).unwrap();
    let files: BTreeSet<String> = conn
        .prepare("SELECT path FROM files")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let expected: BTreeSet<String> = FIXTURES
        .iter()
        .filter(|fixture| fixture.enabled)
        .map(|fixture| fixture.path.to_owned())
        .chain(["README.md".to_owned()])
        .collect();
    assert_eq!(files, expected);
    let docs: usize = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE language='markdown'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(docs > 0);
    let errors: usize = conn
        .query_row("SELECT count(*) FROM errors", [], |row| row.get(0))
        .unwrap();
    assert_eq!(errors, 0);
    let semantic: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_semantics_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(semantic, scanner::INDEX_SEMANTICS_VERSION);
}

#[test]
fn cross_layer_seams_endpoint_vue_alpine() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    fs::create_dir_all(root.join("stores")).unwrap();

    // 1. Python Django backend route + view
    fs::write(
        root.join("urls.py"),
        "from django.urls import path\ndef order_list(request): pass\nurlpatterns = [\n    path('api/v1/orders/', order_list, name='order-list'),\n]\n",
    ).unwrap();

    // 2. TypeScript client calling endpoint via fetch
    fs::write(
        root.join("api.ts"),
        "export async function fetchOrders() {\n    return fetch('/api/v1/orders/');\n}\n",
    )
    .unwrap();

    // 3. HTML calling endpoint via HTMX and Alpine.js component
    fs::write(
        root.join("index.html"),
        concat!(
            "<div>\n",
            "  <button hx-get=\"/api/v1/orders/\">Load</button>\n",
            "  <div x-data=\"{ draft: null, save() { this.draft = 1; } }\">\n",
            "    <button @click=\"save()\">Save</button>\n",
            "  </div>\n",
            "</div>\n",
        ),
    )
    .unwrap();

    // 4. Pinia store
    fs::write(
        root.join("stores/admin.ts"),
        "export function useAdminStore() {\n    return { bookmarks: [] };\n}\n",
    )
    .unwrap();

    // 5. Vue component using auto-imported store and Nuxt built-in
    fs::write(
        root.join("Admin.vue"),
        concat!(
            "<script setup>\n",
            "const adminStore = useAdminStore();\n",
            "navigateTo('/dashboard');\n",
            "</script>\n",
            "<template><div>Admin</div></template>\n",
        ),
    )
    .unwrap();

    let db = root.join(".forge/code-map.sqlite");
    writer::build(root, &db, None).unwrap();
    let conn = reader::open(&db, root).unwrap();

    // Verify calls_endpoint edges
    let endpoint_edges: Vec<String> = conn
        .prepare("SELECT kind FROM edges WHERE kind='calls_endpoint'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !endpoint_edges.is_empty(),
        "calls_endpoint edges must exist"
    );

    // Verify Vue auto-import resolution
    let admin_store_coverage: String = conn
        .query_row(
            "SELECT resolution FROM dependencies WHERE symbol='useAdminStore'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(admin_store_coverage, "resolved");

    // Verify Nuxt builtin resolution (navigateTo -> external)
    let navigate_coverage: String = conn
        .query_row(
            "SELECT resolution FROM dependencies WHERE symbol='navigateTo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(navigate_coverage, "external");

    // Verify Alpine @click handler island was extracted
    let alpine_handler_count: usize = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='template_scope' AND name LIKE 'alpine_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        alpine_handler_count >= 1,
        "Alpine directive islands must be extracted"
    );

    // Verify impact / reachability traversal from order_list reaches client callers
    let view_node_id: String = conn
        .query_row(
            "SELECT id FROM nodes WHERE name='order_list' AND path='urls.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let options = contextunity_forge_mcp::db::traversal::TraversalOptions {
        depth: 4,
        inbound: true,
        mode: None,
        edge_types: None,
        page: &contextunity_forge_mcp::core::response::QueryOptions::resolve(
            &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
            Some(50),
            0,
            Some(contextunity_forge_mcp::core::response::Detail::Compact),
            None,
        )
        .unwrap(),
    };
    let impact = contextunity_forge_mcp::db::traversal::traverse_with_options(
        &conn,
        &view_node_id,
        &options,
    )
    .unwrap();
    let impacted_items = impact["nodes"]["items"].as_array().expect("impact items");
    let impacted_paths: Vec<&str> = impacted_items
        .iter()
        .filter_map(|item| item["path"].as_str())
        .collect();
    assert!(
        impacted_paths.contains(&"api.ts") || impacted_paths.contains(&"index.html"),
        "Inbound impact of backend view must reach frontend callers (api.ts or index.html), got: {:?}",
        impacted_paths
    );
}
