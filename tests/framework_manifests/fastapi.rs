use super::*;

#[cfg(feature = "lang-python")]
#[test]
fn framework_manifest_symbol_relation_links_exact_dependency_injection_target() {
    use contextunity_forge_mcp::engine::languages::manifests::FrameworkManifest;

    let manifest = serde_json::json!({
        "di_calls": [{"callee": "Depends", "argument": 0}],
        "semantic_rules": [{
            "id": "fastapi-dependency",
            "namespace": "fastapi",
            "scope": "project",
            "activation": [{"kind": "package", "name": "fastapi"}],
            "operation": {
                "kind": "call_to_symbol",
                "language": "python",
                "callee": {
                    "kind": "imported",
                    "module": "fastapi",
                    "export": "Depends"
                },
                "argument": 0,
                "target": {
                    "kind": "imported_symbol",
                    "language": "python"
                },
                "relation": "injects_dependency"
            }
        }]
    });
    let source = toml::to_string(&manifest).unwrap();
    let _: FrameworkManifest = toml::from_str(&source).unwrap();
    let mut sources = VirtualWorkspace::from_files(&[
        (
            "pyproject.toml",
            "[project]\nname='custom-symbol-rules'\ndependencies=['fastapi']\n",
        ),
        (
            "app/providers.py",
            "def get_db():\n    return object()\n\ndef ambiguous_db():\n    return object()\n\ndef ambiguous_db():\n    return object()\n",
        ),
        (
            "app/views.py",
            "from fastapi import Depends\nfrom app.providers import get_db, ambiguous_db\n\ndef endpoint(db=Depends(get_db)):\n    return db\n\ndef ambiguous_endpoint(db=Depends(ambiguous_db)):\n    return db\n\ndef missing_endpoint(db=Depends(missing_db)):\n    return db\n",
        ),
        (
            "app/rebound_parameter.py",
            "from fastapi import Depends\nfrom app.providers import get_db\n\ndef endpoint(Depends):\n    return Depends(get_db)\n",
        ),
        (".forge/frameworks/dependencies.toml", &source),
        (
            "unactivated/pyproject.toml",
            "[project]\nname='without-fastapi'\ndependencies=[]\n",
        ),
        (
            "unactivated/views.py",
            "from fastapi import Depends\ndef provider(): return object()\ndef endpoint(db=Depends(provider)): return db\n",
        ),
    ]);
    for (name, rebinding) in [
        ("assignment", "Depends = fake_depends"),
        ("augmented_assignment", "Depends += fake_depends"),
        ("deletion", "del Depends"),
        ("loop_target", "for Depends in (fake_depends,):\n    pass"),
    ] {
        sources.write(
            format!("app/rebound_{name}.py"),
            &format!(
                "from fastapi import Depends\nfrom app.providers import get_db\n\ndef fake_depends(provider):\n    return provider\n\n{rebinding}\n\ndef endpoint(db=Depends(get_db)):\n    return db\n"
            ),
        );
    }

    let conn = sources.index().unwrap().conn;
    let edges: Vec<(String, String, usize)> = fastapi_rows::edges(&conn);
    assert_eq!(
        edges,
        [(
            find_node_id(&conn, "app/views.py", "endpoint"),
            find_node_id(&conn, "app/providers.py", "get_db"),
            4
        )]
    );

    let coverage: Vec<(String, String)> = fastapi_rows::coverage(&conn);
    let actual_coverage: Vec<(&str, &str)> = coverage
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        actual_coverage,
        [
            ("ambiguous_db", "ambiguous"),
            ("get_db", "resolved"),
            ("missing_db", "unresolved")
        ]
    );
    assert_eq!(fastapi_rows::unactivated_dependency_count(&conn), 0);
    assert_eq!(fastapi_rows::rebound_dependency_count(&conn), 0);

    let rebound_coverage: Vec<(String, String)> = fastapi_rows::rebound_coverage(&conn);
    let actual_rebound: Vec<(&str, &str)> = rebound_coverage
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        actual_rebound,
        [
            ("app/rebound_assignment.py", "unresolved"),
            ("app/rebound_augmented_assignment.py", "unresolved"),
            ("app/rebound_deletion.py", "unresolved"),
            ("app/rebound_loop_target.py", "unresolved"),
            ("app/rebound_parameter.py", "unresolved"),
        ]
    );
}
