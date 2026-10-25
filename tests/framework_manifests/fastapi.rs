use super::*;

#[cfg(feature = "lang-python")]
#[test]
fn framework_manifest_symbol_relation_links_exact_dependency_injection_target() {
    use contextunity_forge_mcp::engine::languages::manifests::FrameworkManifest;

    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='custom-symbol-rules'\ndependencies=['fastapi']\n",
    );
    workspace.write(
        "app/providers.py",
        "def get_db():\n    return object()\n\ndef ambiguous_db():\n    return object()\n\ndef ambiguous_db():\n    return object()\n",
    );
    workspace.write(
        "app/views.py",
        "from fastapi import Depends\nfrom app.providers import get_db, ambiguous_db\n\ndef endpoint(db=Depends(get_db)):\n    return db\n\ndef ambiguous_endpoint(db=Depends(ambiguous_db)):\n    return db\n\ndef missing_endpoint(db=Depends(missing_db)):\n    return db\n",
    );
    for (name, rebinding) in [
        ("assignment", "Depends = fake_depends"),
        ("augmented_assignment", "Depends += fake_depends"),
        ("deletion", "del Depends"),
        ("loop_target", "for Depends in (fake_depends,):\n    pass"),
    ] {
        workspace.write(
            format!("app/rebound_{name}.py"),
            &format!(
                "from fastapi import Depends\nfrom app.providers import get_db\n\ndef fake_depends(provider):\n    return provider\n\n{rebinding}\n\ndef endpoint(db=Depends(get_db)):\n    return db\n"
            ),
        );
    }
    workspace.write(
        "app/rebound_parameter.py",
        "from fastapi import Depends\nfrom app.providers import get_db\n\ndef endpoint(Depends):\n    return Depends(get_db)\n",
    );
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
    workspace.write(".forge/frameworks/dependencies.toml", &source);
    workspace.write(
        "unactivated/pyproject.toml",
        "[project]\nname='without-fastapi'\ndependencies=[]\n",
    );
    workspace.write(
        "unactivated/views.py",
        "from fastapi import Depends\ndef provider(): return object()\ndef endpoint(db=Depends(provider)): return db\n",
    );

    let db = workspace.db();
    writer::build(&workspace.0, &db, None).unwrap();
    let conn = reader::open(&db, &workspace.0).unwrap();
    let edges: Vec<(String, String, usize)> = conn
        .prepare("SELECT s.id,d.id,o.line FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash WHERE p.path=?1 AND o.kind=?2 ORDER BY o.line")
        .unwrap().query_map(["app/views.py", "injects_dependency"], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        edges,
        [(
            find_node_id(&conn, "app/views.py", "endpoint"),
            find_node_id(&conn, "app/providers.py", "get_db"),
            4
        )]
    );

    let coverage: Vec<(String, String)> = conn
        .prepare("SELECT x.expression,c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path=?1 AND e.evidence=?2 ORDER BY x.expression")
        .unwrap().query_map(["app/views.py", "declarative imported symbol; exact import and unique callable target required"], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
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
    assert_eq!(query_count(&conn, "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path=?1 AND o.kind=?2", ["unactivated/views.py", "injects_dependency"]), 0);
    assert_eq!(query_count(&conn, "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path LIKE 'app/rebound_%.py' AND o.kind=?1", ["injects_dependency"]), 0);

    let rebound_coverage: Vec<(String, String)> = conn
        .prepare("SELECT p.path,c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path LIKE 'app/rebound_%.py' AND x.expression='get_db' AND e.evidence=?1 ORDER BY p.path")
        .unwrap().query_map(["declarative imported symbol; exact import and unique callable target required"], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
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
