use super::*;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct ForwardingColdCache {
    references: RefCell<Vec<(crate::common::workspace::WorkspaceInputSnapshot, PathBuf)>>,
}

impl Drop for ForwardingColdCache {
    fn drop(&mut self) {
        for (_, path) in self.references.get_mut() {
            let _ = std::fs::remove_file(&*path);
            let _ = std::fs::remove_file(format!("{}-wal", path.display()));
            let _ = std::fs::remove_file(format!("{}-shm", path.display()));
            let _ = std::fs::remove_file(format!("{}.lock", path.display()));
            let _ = std::fs::remove_file(format!("{}.verified.json", path.display()));
        }
    }
}

impl ForwardingColdCache {
    fn verify_parity(&self, w: &PersistedWorkspaceHarness, conn: &Connection, phase: &str) {
        let inputs = w.input_snapshot("forwarding-rust-cold");
        let cached = self
            .references
            .borrow()
            .iter()
            .find(|(inp, _)| inp == &inputs)
            .map(|(_, path)| path.clone());
        let cold = if let Some(path) = cached {
            w.open_at(&path)
        } else {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = w.path(format!(".forge/forwarding-cold-{id}.sqlite"));
            w.build_to(&path);
            commitments::verify(&w.open_at(&path)).unwrap();
            self.references.borrow_mut().push((inputs, path.clone()));
            w.open_at(&path)
        };
        assert_eq!(snapshot(conn), snapshot(&cold), "{phase}");
    }
}

#[test]
fn glob_retarget_remove_and_restore_relink_unchanged_consumers() {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    let frozen = fs::read(w.path("src/glob_consumer.rs")).unwrap();
    for (phase, source, target, status) in [
        (
            "retarget glob",
            "pub use crate::another::*;\n",
            Some(("src/another.rs", "build")),
            "resolved",
        ),
        ("remove glob", "", None, "unresolved"),
        (
            "restore glob",
            GLOB_ONE,
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/glob_one.rs", source);
        w.delta(&["src/glob_one.rs"]);
        let probes = [("src/glob_consumer.rs", "build", target, status)];

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
        cold_cache.verify_parity(&w, &conn, phase);
        assert_eq!(fs::read(w.path("src/glob_consumer.rs")).unwrap(), frozen);
    }
}

type AlternativeCase = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
    &'static str,
);
fn alternative_cases() -> Vec<AlternativeCase> {
    vec![
        (
            "missing becomes same canonical target",
            "missing as partial",
            "build as partial",
            "partial",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "missing becomes a distinct target",
            "missing as partial",
            "other as partial",
            "partial",
            None,
            "ambiguous",
        ),
        (
            "cycle removed",
            "pub use crate::cycle_a::ring as cyclic;",
            "",
            "cyclic",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "inaccessible alternative removed",
            "pub use crate::parent::writer as hidden_mix;",
            "",
            "hidden_mix",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
        (
            "exhausted path shortened",
            "pub use crate::bridge::depth0 as deep_mix;",
            "pub use crate::bridge::depth64 as deep_mix;",
            "deep_mix",
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ]
}

#[test]
fn incomplete_export_alternatives_delta_to_exact_or_ambiguous_targets() {
    assert_incomplete_export_alternatives_delta_cases(&alternative_cases()[..3]);
}

#[test]
fn incomplete_export_alternatives_delta_to_exact_or_ambiguous_targets_part_two() {
    assert_incomplete_export_alternatives_delta_cases(&alternative_cases()[3..]);
}

fn assert_incomplete_export_alternatives_delta_cases(cases: &[AlternativeCase]) {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    let frozen = fs::read(w.path("src/consumer.rs")).unwrap();
    for &(phase, from, to, expression, target, status) in cases {
        w.write("src/facade.rs", &FACADE.replace(from, to));
        w.delta(&["src/facade.rs"]);
        let member = format!("api::{expression}");
        let probes = [
            ("src/consumer.rs", expression, target, status),
            ("src/consumer.rs", member.as_str(), target, status),
        ];

        let conn = w.open();
        commitments::verify(&conn).unwrap();
        assert_eq!(errors(&conn), 0, "{phase}");
        for probe in probes {
            let actual = observe(&conn, &probe);
            assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
            let allowed =
                if phase == "missing becomes a distinct target" && probe.1 == "api::partial" {
                    vec!["unresolved", "ambiguous"]
                } else {
                    vec![probe.3]
                };
            assert!(
                actual
                    .statuses
                    .iter()
                    .all(|status| allowed.contains(&status.as_str())),
                "{phase}: {probe:?}: {:?}",
                actual.statuses
            );
            assert_eq!(actual.canonical.len(), usize::from(probe.2.is_some()));
            assert_eq!(actual.targets, actual.canonical);
            assert_eq!(actual.dependencies, actual.expected_dependencies);
        }
        cold_cache.verify_parity(&w, &conn, phase);
        assert_eq!(fs::read(w.path("src/consumer.rs")).unwrap(), frozen);
        w.write("src/facade.rs", FACADE);
        w.delta(&["src/facade.rs"]);
    }
}

#[test]
fn restored_incomplete_alternatives_recover_every_initial_binding() {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    let phase = "restore incomplete alternatives";
    for (_, from, to, _, _, _) in alternative_cases() {
        w.write("src/facade.rs", &FACADE.replace(from, to));
        w.delta(&["src/facade.rs"]);
        w.write("src/facade.rs", FACADE);
        w.delta(&["src/facade.rs"]);

        let conn = w.open();
        commitments::verify(&conn).unwrap();
        assert_eq!(errors(&conn), 0, "{phase}");
        for probe in initial_probes() {
            let actual = observe(&conn, &probe);
            assert!(!actual.statuses.is_empty(), "{phase}: {probe:?}");
            assert!(actual.statuses.iter().all(|status| status == probe.3));
            assert_eq!(actual.canonical.len(), usize::from(probe.2.is_some()));
            assert_eq!(actual.targets, actual.canonical);
            assert_eq!(actual.dependencies, actual.expected_dependencies);
        }
    }
    let conn = w.open();
    cold_cache.verify_parity(&w, &conn, phase);
}

#[test]
fn named_export_retarget_remove_restore_relinks_all_forwarders() {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    for (phase, replacement, target, status) in [
        (
            "retarget export",
            FACADE.replacen("build as forwarded", "other as forwarded", 1),
            Some(("src/provider.rs", "other")),
            "resolved",
        ),
        (
            "remove export",
            FACADE.replacen("build as forwarded, ", "", 1),
            None,
            "unresolved",
        ),
        (
            "restore export",
            FACADE.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/facade.rs", &replacement);
        w.delta(&["src/facade.rs"]);
        let probes = [
            ("src/consumer.rs", "invoke", target, status),
            ("src/consumer.rs", "api::forwarded", target, status),
            ("src/parent/child.rs", "inherited", target, status),
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
        cold_cache.verify_parity(&w, &conn, phase);
    }
}

#[test]
fn parent_alias_rename_and_restore_relinks_child() {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    for (phase, replacement, target, status) in [
        (
            "rename parent alias",
            PARENT.replace("as writer", "as renamed"),
            None,
            "unresolved",
        ),
        (
            "restore parent alias",
            PARENT.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/parent/mod.rs", &replacement);
        w.delta(&["src/parent/mod.rs"]);
        let probes = [("src/parent/child.rs", "emit", target, status)];

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
        cold_cache.verify_parity(&w, &conn, phase);
    }
}

#[test]
fn provider_remove_and_restore_relinks_transitive_consumers() {
    let w = forwarding_workspace();
    let cold_cache = ForwardingColdCache::default();
    for (phase, replacement, target, status) in [
        (
            "remove provider",
            PROVIDER.replace("pub fn build() {}\n", ""),
            None,
            "unresolved",
        ),
        (
            "restore provider",
            PROVIDER.to_owned(),
            Some(("src/provider.rs", "build")),
            "resolved",
        ),
    ] {
        w.write("src/provider.rs", &replacement);
        w.delta(&["src/provider.rs"]);
        let probes = [
            ("src/consumer.rs", "invoke", target, status),
            ("src/parent/child.rs", "inherited", target, status),
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
        cold_cache.verify_parity(&w, &conn, phase);
    }
}
