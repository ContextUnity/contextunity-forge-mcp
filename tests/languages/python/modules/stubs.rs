use super::*;
#[test]
fn paired_runtime_and_stub_choose_runtime_for_each_import_form() {
    for (consumer, expression, expected_paths, check_stub) in [
        (
            "from pkg import child\ndef consume(): return child.value()\n",
            "child",
            &["pkg/__init__.py", "pkg/child.py"][..],
            true,
        ),
        (
            "import pkg.child as child\ndef consume(): return child.value()\n",
            "pkg.child",
            &["pkg/child.py"][..],
            false,
        ),
    ] {
        let w = fixture_workspace(&[
            ("pkg/__init__.py", "# package\n"),
            ("pkg/child.py", "def value(): return 1\n"),
            ("pkg/child.pyi", "def value() -> int: ...\n"),
            ("consumer.py", consumer),
        ]);
        w.build();
        let conn = w.open();
        assert_eq!(status(&conn, expression), "resolved");
        assert_eq!(import_target_paths(&conn), expected_paths);
        assert_eq!(call_targets(&conn), ["pkg.child.value"]);
        if check_stub {
            let stub: i64 = query_scalar(
                &conn,
                "SELECT count(*) FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='pkg/child.pyi')",
                [],
            );
            assert!(stub > 0);
        }
    }
}

#[test]
fn paired_stub_supplies_declaration_missing_from_generated_runtime() {
    let w = fixture_workspace(&[
        ("pkg/__init__.py", "# package\n"),
        ("pkg/child.py", "def __getattr__(name): return None\n"),
        (
            "pkg/child.pyi",
            "class ContextUnit:\n    def value(self) -> int: ...\n",
        ),
        (
            "consumer.py",
            "from pkg.child import ContextUnit\ndef consume(): return ContextUnit()\n",
        ),
    ]);
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "ContextUnit"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/child.py", "pkg/child.pyi"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
    let evidence: String = query_scalar(
        &conn,
        "SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ContextUnit'",
        [],
    );
    assert!(evidence.contains("type stub"), "{evidence}");
    drop(conn);
    w.write("pkg/child.py", "def __getattr__(name): return name\n");
    w.delta(&["pkg/child.py"]);
    let parity = crate::languages::harness::cold_relations(&w);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.delta(&["pkg/child.py"]);
    let parity = crate::languages::harness::cold_relations(&w);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
}

#[test]
fn child_module_call_uses_stub_only_member() {
    let w = fixture_workspace(&[
        ("pkg/__init__.py", "# package\n"),
        ("pkg/child.py", "def __getattr__(name): return None\n"),
        ("pkg/child.pyi", "class ContextUnit: ...\n"),
        (
            "consumer.py",
            "from pkg import child\ndef consume(): return child.ContextUnit()\n",
        ),
    ]);
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.py"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
}

#[test]
fn stub_only_module_remains_navigable() {
    let w = fixture_workspace(&[
        ("pkg/__init__.py", "# package\n"),
        ("pkg/child.pyi", "def value() -> int: ...\n"),
        (
            "consumer.py",
            "from pkg import child\ndef consume(): return child.value()\n",
        ),
    ]);
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.pyi"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn child_module_candidates_must_be_unique_and_related() {
    for (label, providers, expected_status, expected_imports) in [
        (
            "multiple runtime modules",
            &[
                ("pkg/child.py", "def value(): return 1\n"),
                ("src/pkg/child.py", "def value(): return 2\n"),
            ][..],
            "ambiguous",
            &["pkg/__init__.py"][..],
        ),
        (
            "unrelated child module",
            &[("other/child.py", "def value(): return 1\n")][..],
            "unresolved",
            &["pkg"][..],
        ),
    ] {
        let mut files = vec![("pkg/__init__.py", "# package\n")];
        files.extend_from_slice(providers);
        files.push((
            "consumer.py",
            "from pkg import child\ndef consume(): return child.value()\n",
        ));
        let w = fixture_workspace(&files);
        w.build();
        let conn = w.open();
        assert_eq!(status(&conn, "child"), expected_status, "{label}");
        if label == "multiple runtime modules" {
            assert_eq!(import_target_paths(&conn), ["pkg/__init__.py"], "{label}");
        } else {
            assert_eq!(import_targets(&conn), expected_imports, "{label}");
        }
        assert!(call_targets(&conn).is_empty(), "{label}");
    }
}

#[test]
fn package_symbol_takes_precedence_over_same_named_child_module() {
    let w = fixture_workspace(&[
        ("pkg/__init__.py", "def child(): return 1\n"),
        ("pkg/child.py", "def value(): return 2\n"),
        (
            "consumer.py",
            "from pkg import child\ndef consume(): return child()\n",
        ),
    ]);
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(import_targets(&conn), ["pkg", "pkg.child"]);
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/__init__.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child"]);
}

#[test]
fn child_edit_and_restore_match_cold_graph() {
    let w = fixture_workspace(&[
        ("pkg/__init__.py", "# package\n"),
        ("pkg/child.py", "def value(): return 1\n"),
        ("pkg/child.pyi", "def value() -> int: ...\n"),
        (
            "consumer.py",
            "from pkg import child\ndef consume(): return child.value()\n",
        ),
    ]);
    w.build();
    w.write("pkg/child.py", "def value(arg=1): return arg\n");
    w.delta(&["pkg/child.py"]);
    let parity = crate::languages::harness::cold_relations(&w);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    w.write("pkg/child.py", "def value(): return 1\n");
    w.delta(&["pkg/child.py"]);
    let parity = crate::languages::harness::cold_relations(&w);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
}
