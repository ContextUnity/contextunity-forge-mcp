const ROOT_IMPORT_CASES: &[(&str, &str, bool, i64)] = &[
    ("[package]\nname='root_fixture'\n", "root_fixture", true, 5),
    ("[package]\nname='root-fixture'\n", "root_fixture", true, 5),
    (
        "[package]\nname='root-fixture'\n[lib]\nname='custom_lib'\n",
        "custom_lib",
        true,
        5,
    ),
    (
        "[package]\nname='root-fixture'\n[lib]\nname='custom_lib'\n",
        "root_fixture",
        true,
        0,
    ),
    ("[package]\nname='root-fixture'\n", "other_fixture", true, 0),
    (
        "[package]\nname='root-fixture'\n",
        "root_fixture_extra",
        true,
        0,
    ),
    ("[workspace]\nmembers=[]\n", "root_fixture", true, 0),
    ("[package\nname='root-fixture'\n", "root_fixture", true, 0),
    ("[package]\nname=[]\n", "root_fixture", true, 0),
    ("", "root_fixture", true, 0),
    ("[package]\nname='root-fixture'\n", "root_fixture", false, 0),
    (
        "[package]\nname='root-fixture'\n[lib]\npath='actual/lib.rs'\n",
        "root_fixture",
        true,
        0,
    ),
    (
        "[package]\nname='root-fixture'\nautolib=false\n",
        "root_fixture",
        true,
        0,
    ),
    (
        "[package]\nname='root-fixture'\nautolib=false\n[lib]\npath='src/lib.rs'\n",
        "root_fixture",
        true,
        5,
    ),
    (
        "[package]\nname='root-fixture'\nautolib=false\n[lib]\n",
        "root_fixture",
        true,
        5,
    ),
];

use super::*;

#[cfg(feature = "lang-go")]
#[test]
fn go_sibling_functions_in_same_package_link() {
    let w = PersistedWorkspaceHarness::new();
    w.write(
        "pkg/helper.go",
        r#"package mypkg

func Helper() int {
    return 42
}
"#,
    );
    w.write(
        "pkg/main.go",
        r#"package mypkg

func Main() int {
    return Helper()
}
"#,
    );
    w.build();
    let calls: Vec<_> = w
        .calls()
        .into_iter()
        .filter(|call| call.kind == "calls")
        .map(|call| (call.caller_name, call.target_name))
        .collect();
    assert_eq!(calls, [("Main".into(), "Helper".into())]);

    let resolved = w
        .coverage()
        .into_iter()
        .filter(|row| row.expression == "Helper" && row.status == "resolved")
        .count();
    assert_eq!(resolved, 1, "expected Helper to be resolved");
}

#[cfg(feature = "lang-rust")]
#[test]
fn root_cargo_crate_imports_link_integration_tests_and_preserve_delta_parity() {
    check_root_cargo_crate_import_cases(&ROOT_IMPORT_CASES[..4]);
}

#[cfg(feature = "lang-rust")]
#[test]
fn root_cargo_crate_imports_reject_mismatched_crate_names_and_preserve_delta_parity() {
    check_root_cargo_crate_import_cases(&ROOT_IMPORT_CASES[4..8]);
}

#[cfg(feature = "lang-rust")]
#[test]
fn root_cargo_crate_imports_reject_invalid_manifests_and_binary_only_roots_and_preserve_delta_parity() {
    check_root_cargo_crate_import_cases(&ROOT_IMPORT_CASES[8..12]);
}

#[cfg(feature = "lang-rust")]
#[test]
fn root_cargo_crate_imports_respect_custom_lib_path_and_autolib_settings_and_preserve_delta_parity() {
    check_root_cargo_crate_import_cases(&ROOT_IMPORT_CASES[12..]);
}

#[cfg(feature = "lang-rust")]
fn check_root_cargo_crate_import_cases(cases: &[(&str, &str, bool, i64)]) {
    for &(manifest, crate_name, library, expected) in cases {
        let w = PersistedWorkspaceHarness::new();
        if !manifest.is_empty() {
            w.write("Cargo.toml", manifest);
        }
        w.write(
            if library { "src/lib.rs" } else { "src/main.rs" },
            "pub mod api;\npub fn root_route() {}\n",
        );
        w.write("src/api.rs", "pub fn route() -> usize { 1 }\n");
        if manifest.contains("actual/lib.rs") {
            w.write("actual/lib.rs", "pub fn actual() {}\n");
        }
        w.write(
            "tests/connected.rs",
            &format!("use {crate_name}::api::route;\nuse {crate_name}::api::route as alias;\nuse {crate_name}::api;\nuse {crate_name}::root_route;\n#[test]\nfn direct() {{ let actual = route(); assert_eq!(actual, 1); }}\n#[test]\nfn aliased() {{ alias(); }}\n#[test]\nfn namespaced() {{ api::route(); }}\n#[test]\nfn root_symbol() {{ root_route(); }}\nmod glob {{ use {crate_name}::api::*; #[test] fn globbed() {{ route(); }} }}\n"),
        );
        w.build();
        let calls = w
            .calls()
            .into_iter()
            .filter(|call| {
                call.kind == "calls"
                    && call.caller_is_test
                    && call.caller_path == "tests/connected.rs"
                    && ["route", "root_route"].contains(&call.target_name.as_str())
                    && ["src/api.rs", "src/lib.rs", "src/main.rs"]
                        .contains(&call.target_path.as_str())
            })
            .count() as i64;
        assert_eq!(
            calls, expected,
            "Cargo import {crate_name}, library={library}, manifest={manifest}"
        );
        w.write("src/api.rs", "pub fn route() -> usize { 2 }\n");
        let parity = w.delta_cold("src/api.rs");
        assert_eq!(parity.incremental_verification, Ok(()));
        assert_eq!(parity.cold_verification, Ok(()));
        assert_eq!(parity.incremental, parity.cold);
        if expected > 0 {
            w.write("Cargo.toml", "[package]\nname='renamed-fixture'\n");
            let parity = w.delta_cold("Cargo.toml");
            assert_eq!(parity.incremental_verification, Ok(()));
            assert_eq!(parity.cold_verification, Ok(()));
            assert_eq!(parity.incremental, parity.cold);
            let stale = w
                .calls()
                .into_iter()
                .filter(|call| {
                    call.kind == "calls"
                        && call.caller_is_test
                        && ["route", "root_route"].contains(&call.target_name.as_str())
                })
                .count();
            assert_eq!(
                stale, 0,
                "Cargo rename invalidates the former crate identity"
            );
        }
    }
}

#[test]
fn nested_cargo_package_cannot_borrow_root_crate_identity() {
    let w = PersistedWorkspaceHarness::new();
    w.write("Cargo.toml", "[package]\nname='root-fixture'\n");
    w.write("src/lib.rs", "pub fn route() {}\n");
    w.write("nested/Cargo.toml", "[package]\nname='nested-fixture'\n");
    w.write(
        "nested/tests/connected.rs",
        "use root_fixture::route;\n#[test]\nfn connected() { route(); }\n",
    );
    w.build();
    let calls = w
        .calls()
        .into_iter()
        .filter(|call| {
            call.kind == "calls"
                && call.caller_is_test
                && call.caller_path == "nested/tests/connected.rs"
                && call.caller_name == "connected"
                && call.target_name == "route"
        })
        .count();
    assert_eq!(
        calls, 0,
        "a nested Cargo package cannot borrow the root crate identity"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_macros_and_cargo_workspace() {
    let w = PersistedWorkspaceHarness::new();
    w.write(
        "crates/core/src/macros.rs",
        r#"#[macro_export]
macro_rules! log_info {
    ($msg:expr) => {
        println!("{}", $msg)
    };
}

"#,
    );
    w.write(
        "crates/core/src/lib.rs",
        r#"pub mod macros;

use crate::macros::log_info;

pub fn execute() {
    log_info!("running");
    println!("standard macro");
}
"#,
    );

    w.build();
    let connection = w.open();
    let macro_count: i64 = crate::languages::harness::query_scalar(
        &connection,
        "SELECT count(*) FROM nodes WHERE kind='macro' AND name='log_info'",
        [],
    );
    assert_eq!(macro_count, 1);

    // Verify standard-library println retains external provenance.
    let coverage = w.coverage();
    let println_external = coverage
        .iter()
        .filter(|row| row.expression == "println" && row.status == "external")
        .count();
    assert_eq!(println_external, 1);

    // Verify crate:: import resolved to crates.core.src prefix
    let import_resolved = coverage
        .iter()
        .filter(|row| row.expression.contains("log_info") && row.status == "resolved")
        .count();
    assert!(
        import_resolved >= 1,
        "expected crate:: import of log_info to be resolved"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_manifest_types_preserve_receiver_origin_for_aliases_and_qualified_names() {
    let w = PersistedWorkspaceHarness::new();
    w.write("Cargo.toml", "[package]\nname = \"receiver-provenance\"\nversion = \"0.1.0\"\n[dependencies]\nstorage-api = \"1\"\n");
    w.write("src/lib.rs", "use storage_api::Connection as DbConnection;\nfn imported(conn: &DbConnection) { conn.query(); }\nfn qualified(conn: &storage_api::Connection) { conn.query(); }\nfn generic(conn: &storage_api::Connection<u8>) { conn.query(); }\nfn associated(conn: &storage_api::Connection<u8>::Item<u8>) { conn.query(); }\nstruct Local;\nfn local(value: &Local) { value.query(); }\nfn undeclared(value: &missing_api::Connection) { value.query(); }\n");
    w.build();
    let rows: Vec<_> = w
        .coverage()
        .into_iter()
        .filter(|row| row.expression == "conn.query" || row.expression == "value.query")
        .collect();
    for line in [2, 3, 4] {
        assert!(
            rows.iter().any(|row| row.line == line
                && row.expression == "conn.query"
                && row.status == "external"
                && row.evidence.contains("storage_api")),
            "{rows:#?}"
        );
    }
    assert!(
        rows.iter().any(|row| row.line == 5
            && row.expression == "conn.query"
            && row.status == "unresolved"),
        "{rows:#?}"
    );
    for line in [7, 8] {
        assert!(
            rows.iter().any(|row| row.line == line
                && row.expression == "value.query"
                && row.status == "unresolved"),
            "{rows:#?}"
        );
    }
}
