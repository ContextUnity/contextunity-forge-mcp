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
