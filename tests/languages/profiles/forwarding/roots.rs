use super::*;

fn root_workspace(manifest: &str, library: bool) -> PersistedWorkspaceHarness {
    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write("Cargo.toml", manifest);
    if library {
        w.write("src/lib.rs", "pub fn build() {}\npub struct Workspace;\nimpl Workspace { pub fn new() -> Self { Self } }\n");
    }
    w.write("foreign/Cargo.toml", "[package]\nname='root-kit'\n");
    w.write("foreign/src/lib.rs", "pub fn build() {}\n");
    w.write("tests/api.rs", "use root_kit::{build as execute, Workspace};\nfn probe() {\n    execute();\n    Workspace::new();\n}\n");
    w.build();
    w
}

type RootCase = (&'static str, &'static str, bool, bool);

fn root_cases() -> Vec<RootCase> {
    vec![
        ("package name", "[package]\nname='root-kit'\n", true, true),
        (
            "explicit library name",
            "[package]\nname='different'\n[lib]\nname='root_kit'\n",
            true,
            true,
        ),
        (
            "wrong package",
            "[package]\nname='other-kit'\n",
            true,
            false,
        ),
        (
            "absent library",
            "[package]\nname='root-kit'\n",
            false,
            false,
        ),
        (
            "disabled library",
            "[package]\nname='root-kit'\nautolib=false\n",
            true,
            false,
        ),
        (
            "missing package name",
            "[workspace]\n[lib]\nname='root_kit'\n",
            true,
            false,
        ),
        (
            "inherited package name",
            "[package]\nname.workspace=true\n[lib]\nname='root_kit'\n",
            true,
            false,
        ),
        (
            "explicit enabled library",
            "[package]\nname='root-kit'\nautolib=false\n[lib]\n",
            true,
            true,
        ),
        (
            "unsupported custom path",
            "[package]\nname='root-kit'\n[lib]\npath='custom/lib.rs'\n",
            true,
            false,
        ),
    ]
}

#[test]
fn cargo_root_identity_requires_supported_package_and_library_metadata() {
    assert_cargo_root_identity_cases(&root_cases()[..5]);
}

#[test]
fn cargo_root_identity_requires_supported_package_and_library_metadata_rejections() {
    assert_cargo_root_identity_cases(&root_cases()[5..]);
}

fn assert_cargo_root_identity_cases(cases: &[RootCase]) {
    for &(phase, manifest, library, expected) in cases {
        let w = root_workspace(manifest, library);
        let conn = w.open();
        commitments::verify(&conn).unwrap();
        assert_eq!(errors(&conn), 0, "{phase}");
        let targets = if expected {
            Some(("src/lib.rs", "build"))
        } else {
            None
        };
        let constructors = if expected {
            Some(("src/lib.rs", "new"))
        } else {
            None
        };
        let probes = [
            ("tests/api.rs", "execute", targets, "resolved"),
            ("tests/api.rs", "Workspace::new", constructors, "resolved"),
        ];
        if expected || phase == "missing package name" || phase == "inherited package name" {
            for probe in probes {
                let actual = observe(&conn, &probe);
                let expected_status = if expected { "resolved" } else { "external" };
                assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
                assert!(
                    actual
                        .statuses
                        .iter()
                        .all(|status| status == expected_status),
                    "{phase}: {probe:?}: {:?}",
                    actual.statuses
                );
                assert_eq!(actual.canonical.len(), usize::from(expected));
                assert_eq!(actual.targets, actual.canonical);
                assert_eq!(actual.dependencies, actual.expected_dependencies);
            }
        }
        drop(conn);
        if expected {
            w.write("Cargo.toml", "[package]\nname='renamed-kit'\n");
            w.delta(&["Cargo.toml"]);
        }
        let calls: i64 = w.open().query_row("SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path='tests/api.rs' AND o.kind='calls'", [], |r| r.get(0)).unwrap();
        assert_eq!(calls, 0, "{phase}: absent or renamed root identity");
        let cold = fresh_cold(&w);
        commitments::verify(&cold).unwrap();
        assert_eq!(snapshot(&w.open()), snapshot(&cold), "{phase}");
    }
}

#[test]
fn restored_root_identity_recovers_canonical_function_and_constructor_targets() {
    for (phase, manifest, library, expected) in root_cases() {
        if !expected {
            continue;
        }
        let w = root_workspace(manifest, library);
        w.write("Cargo.toml", "[package]\nname='renamed-kit'\n");
        w.delta(&["Cargo.toml"]);
        w.write("Cargo.toml", manifest);
        w.delta(&["Cargo.toml"]);
        let probes = [
            (
                "tests/api.rs",
                "execute",
                Some(("src/lib.rs", "build")),
                "resolved",
            ),
            (
                "tests/api.rs",
                "Workspace::new",
                Some(("src/lib.rs", "new")),
                "resolved",
            ),
        ];

        let conn = w.open();
        commitments::verify(&conn).unwrap();
        assert_eq!(errors(&conn), 0, "{phase}");
        for probe in probes {
            let actual = observe(&conn, &probe);
            assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
            assert!(actual.statuses.iter().all(|status| status == probe.3));
            assert_eq!(actual.canonical.len(), usize::from(probe.2.is_some()));
            assert_eq!(actual.targets, actual.canonical);
            assert_eq!(actual.dependencies, actual.expected_dependencies);
        }
        let cold = fresh_cold(&w);
        commitments::verify(&cold).unwrap();
        assert_eq!(snapshot(&conn), snapshot(&cold), "{phase}");
    }
}

fn nested_consumer_workspace() -> PersistedWorkspaceHarness {
    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write("Cargo.toml", "[package]\nname='root-kit'\n");
    w.write("src/lib.rs", "pub mod provider;\n");
    w.write("src/provider.rs", "pub fn build() {}\n");
    w.write("foreign/Cargo.toml", "[package]\nname='foreign-kit'\n");
    w.write("foreign/src/lib.rs", "pub mod provider;\nmod consumer;\n");
    w.write("foreign/src/provider.rs", "pub fn build() {}\n");
    for path in ["src/consumer.rs", "foreign/src/consumer.rs"] {
        w.write(path, "use crate::provider::build as from_crate;\nuse super::provider::build as from_super;\nuse self::local as own_alias;\nuse missing_api::build as external;\nfn local() {}\nfn probe() {\n    from_crate();\n    from_super();\n    own_alias();\n    external();\n}\n");
    }
    w.write(
        "foreign/tests/probe.rs",
        "use root_kit::provider::build as execute;\nfn probe() { execute(); }\n",
    );
    w.write(
        "tests/control.rs",
        "use root_kit::provider::build as execute;\nfn probe() { execute(); }\n",
    );
    w.build();
    w
}

#[test]
fn nested_consumer_package_cannot_acquire_root_library_identity() {
    let w = nested_consumer_workspace();
    let phase = "nested consumer package boundary";
    let probes = [
        (
            "tests/control.rs",
            "execute",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        ("foreign/tests/probe.rs", "execute", None, "unresolved"),
    ];

    let conn = w.open();
    commitments::verify(&conn).unwrap();
    assert_eq!(errors(&conn), 0, "{phase}");
    for probe in probes {
        let actual = observe(&conn, &probe);
        assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
        assert!(actual.statuses.iter().all(|status| status == probe.3));
        assert_eq!(actual.canonical.len(), usize::from(probe.2.is_some()));
        assert_eq!(actual.targets, actual.canonical);
        assert_eq!(actual.dependencies, actual.expected_dependencies);
    }
    let cold = fresh_cold(&w);
    commitments::verify(&cold).unwrap();
    assert_eq!(snapshot(&conn), snapshot(&cold), "{phase}");
}

#[test]
fn relative_and_self_imports_keep_package_identity_across_root_metadata_deltas() {
    let w = nested_consumer_workspace();
    for (phase, manifest) in [
        ("valid root metadata", "[package]\nname='root-kit'\n"),
        ("missing root metadata", "[workspace]\n"),
        (
            "inherited root metadata",
            "[package]\nname.workspace=true\n",
        ),
        ("restored root metadata", "[package]\nname='root-kit'\n"),
    ] {
        w.write("Cargo.toml", manifest);
        w.delta(&["Cargo.toml"]);
        for (owner, provider) in [
            ("src/consumer.rs", "src/provider.rs"),
            ("foreign/src/consumer.rs", "foreign/src/provider.rs"),
        ] {
            let probes = [
                (owner, "from_crate", Some((provider, "build")), "resolved"),
                (owner, "from_super", Some((provider, "build")), "resolved"),
                (owner, "own_alias", Some((owner, "local")), "resolved"),
                (owner, "external", None, "external"),
            ];

            let conn = w.open();
            commitments::verify(&conn).unwrap();
            assert_eq!(errors(&conn), 0, "{phase}");
            for probe in probes {
                let actual = observe(&conn, &probe);
                assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
                assert!(actual.statuses.iter().all(|status| status == probe.3));
                assert_eq!(actual.canonical.len(), usize::from(probe.2.is_some()));
                assert_eq!(actual.targets, actual.canonical);
                assert_eq!(actual.dependencies, actual.expected_dependencies);
            }
        }
        let conn = w.open();
        let cold = fresh_cold(&w);
        commitments::verify(&cold).unwrap();
        assert_eq!(snapshot(&conn), snapshot(&cold), "{phase}");
    }
}
