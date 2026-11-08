use super::*;

const MANIFESTS: &[(&str, &str)] = &[
    ("requirements.txt", "django\nfastapi\n"),
    ("nested/requirements.txt", ""),
    (
        "package.json",
        "{\"name\":\"primary\",\"dependencies\":{\"vue\":\"3\"}}",
    ),
    (
        "package-lock.json",
        "{\"lockfileVersion\":3,\"packages\":{\"node_modules/vue\":{\"version\":\"3\"}}}",
    ),
    (
        ".forge/frameworks/django.toml",
        "[[route_calls]]\ncallee='LocalRoute'\nmethod='GET'\npath_argument=0\nhandler_argument=1\n",
    ),
    (".forge/frameworks/fastapi.yaml", "enabled: true\n"),
];
const SOURCES: &[(&str, &str, &str)] = &[
    ("app.py", "python", "from fastapi import FastAPI\nclass Record: pass\napp=FastAPI()\n@app.get('/records/', response_model=Record)\ndef records(): return []\nLocalRoute('/local/', records)\n"),
    ("nested/app.py", "python", "class Record: pass\nclass View:\n    model=Record\n"),
];

#[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
#[test]
fn source_registry_matches_disk_scopes_facts_and_linked_graph() {
    let workspace = FrameworkHarness::persisted(&[]);
    let linked = FrameworkHarness::persisted(&[]);
    for name in [
        "my-service",
        "two words",
        "команда",
        "a/b",
        "[other]",
        "../other",
    ] {
        let (source, disk) =
            FrameworkHarness::registry_pair_in(&workspace, &linked, name, MANIFESTS);
        assert_eq!(source.digest(), disk.digest());
        for (path, pack) in [
            ("app.py", "django"),
            ("nested/app.py", "django"),
            (&format!("[{name}]/app.py"), "fastapi"),
        ] {
            assert_eq!(
                FrameworkHarness::effective_manifest(&source, path, pack),
                FrameworkHarness::effective_manifest(&disk, path, pack)
            );
        }
        let source_facts = FrameworkHarness::extract_with_registry(&source, SOURCES).unwrap();
        let disk_facts = FrameworkHarness::extract_with_registry(&disk, SOURCES).unwrap();
        for (path, facts) in &source_facts {
            assert_eq!(
                serde_json::to_value(facts.clone().into_public()).unwrap(),
                serde_json::to_value(disk_facts[path].clone().into_public()).unwrap()
            );
        }
        let actual: contextunity_forge_mcp::core::models::Graph =
            linker::link_compact_with_typed_registry(&source_facts, None, None, &source).into();
        let expected: contextunity_forge_mcp::core::models::Graph =
            linker::link_compact_with_typed_registry(&disk_facts, None, None, &disk).into();
        assert_eq!(
            serde_json::to_value(&actual.edges).unwrap(),
            serde_json::to_value(expected.edges).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&actual.coverage).unwrap(),
            serde_json::to_value(expected.coverage).unwrap()
        );
        let nodes = &source_facts["app.py"].facts.nodes;
        let route = nodes
            .iter()
            .find(|node| node.name == "GET /local/")
            .unwrap();
        let handler = nodes.iter().find(|node| node.name == "records").unwrap();
        assert!(actual
            .edges
            .iter()
            .any(|edge| edge.kind == "handles" && edge.src == route.id && edge.dst == handler.id));
        assert!(actual.edges.iter().any(|edge| edge.kind == "uses_model"));
        assert!(!actual
            .edges
            .iter()
            .any(|edge| edge.kind == "uses_model" && edge.src.contains("nested/app.py")));
    }
}

#[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
#[test]
fn source_registry_matches_disk_linked_scope_canonical_targets() {
    let workspace = FrameworkHarness::persisted(&[]);
    let linked = FrameworkHarness::persisted(&[]);
    for name in ["my-service", "two words", "команда", "[other]"] {
        let (source, disk) =
            FrameworkHarness::registry_pair_in(&workspace, &linked, name, MANIFESTS);
        let app = format!("[{name}]/app.py");
        let nested = format!("[{name}]/nested/app.py");
        let files = [
            (app.as_str(), "python", SOURCES[0].2),
            (nested.as_str(), "python", SOURCES[1].2),
        ];
        let actual_facts = FrameworkHarness::extract_with_registry(&source, &files).unwrap();
        let expected_facts = FrameworkHarness::extract_with_registry(&disk, &files).unwrap();
        let actual = linker::link_compact_with_typed_registry(&actual_facts, None, None, &source);
        let expected = linker::link_compact_with_typed_registry(&expected_facts, None, None, &disk);
        let actual: contextunity_forge_mcp::core::models::Graph = actual.into();
        let expected: contextunity_forge_mcp::core::models::Graph = expected.into();
        assert_eq!(
            serde_json::to_value(&actual.edges).unwrap(),
            serde_json::to_value(&expected.edges).unwrap(),
            "{name}"
        );
        assert_eq!(
            serde_json::to_value(&actual.coverage).unwrap(),
            serde_json::to_value(&expected.coverage).unwrap(),
            "{name}"
        );
        let nodes = &actual_facts[&app].facts.nodes;
        let route = nodes
            .iter()
            .find(|node| node.name == "GET /records/")
            .unwrap();
        let handler = nodes.iter().find(|node| node.name == "records").unwrap();
        let model = nodes.iter().find(|node| node.name == "Record").unwrap();
        for (kind, src, dst) in [
            ("handles", &route.id, &handler.id),
            ("uses_model", &handler.id, &model.id),
        ] {
            assert_eq!(
                actual
                    .edges
                    .iter()
                    .filter(|edge| edge.kind == kind && edge.src == *src && edge.dst == *dst)
                    .count(),
                1,
                "{name}: exact linked {kind} target"
            );
        }
        let view = actual_facts[&nested]
            .facts
            .nodes
            .iter()
            .find(|node| node.name == "View")
            .unwrap();
        assert!(
            !actual
                .edges
                .iter()
                .any(|edge| edge.kind == "uses_model" && edge.src == view.id),
            "{name}: linked FastAPI scope must not activate primary Django policy"
        );
    }
}
