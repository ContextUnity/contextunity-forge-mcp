use super::*;
use contextunity_forge_mcp::core::commitments;
use rusqlite::{params, Connection};

mod deltas;
mod roots;

pub(super) type Probe<'a> = (&'a str, &'a str, Option<(&'a str, &'a str)>, &'a str);

pub(super) const PROVIDER: &str = "pub fn build() {}\npub fn other() {}\nfn hidden() {}\npub struct Workspace;\nimpl Workspace { pub fn new() -> Self { Self } pub fn work(&self) {} }\n";
pub(super) const PARENT: &str = "use crate::provider::{build as writer, Workspace};\nuse crate::bridge::twice as inherited;\nmod child;\n";
pub(super) const GLOB_ONE: &str = "pub use crate::provider::*;\n";
pub(super) const FACADE: &str = "pub use crate::provider::{build as forwarded, Workspace as Room};\npub use crate::provider as tools;\npub use crate::provider::missing as absent;\npub use crate::provider::build as conflict;\npub use crate::provider::other as conflict;\npub use crate::parent::writer as leaked;\npub use crate::provider::build as partial;\npub use crate::provider::missing as partial;\npub use crate::provider::build as cyclic;\npub use crate::cycle_a::ring as cyclic;\npub use crate::provider::build as same;\npub use crate::provider::build as same;\npub use crate::provider::build as hidden_mix;\npub use crate::parent::writer as hidden_mix;\npub use crate::provider::build as deep_mix;\npub use crate::bridge::depth0 as deep_mix;\n";

pub(super) fn forwarding_workspace() -> PersistedWorkspaceHarness {
    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write("src/lib.rs", "pub mod provider;\nmod parent;\npub mod facade;\npub mod bridge;\nmod consumer;\nmod cycle_a;\nmod cycle_b;\nmod a;\nmod b;\npub mod another;\npub mod glob_one;\npub mod glob_two;\npub mod glob_ambiguous;\npub mod glob_same;\npub mod glob_mixed;\npub mod glob_outer;\npub mod glob_cycle;\npub mod glob_missing;\nmod glob_consumer;\nmod glob_shadow;\nmod glob_named;\nmod glob_broken;\nmod glob_negative;\nmod lexical_direct;\nmod lexical_typed;\nmod lexical_transitive;\nmod lexical_block;\nmod lexical_named;\nmod lexical_broken;\nmod lexical_own;\nmod lexical_private;\nmod lexical_ambiguous;\nmod lexical_incomplete;\nmod module_facade;\nmod module_outer;\nmod module_direct;\nmod module_forwarded;\n#[path = \"other.rs\"] mod attributed;\n#[cfg(any())] mod disabled;\nfn declared_probe() {\n    provider::build();\n    undeclared::build();\n    attributed::build();\n    disabled::build();\n}\n");
    w.write("src/provider.rs", PROVIDER);
    for path in ["src/undeclared.rs", "src/attributed.rs", "src/disabled.rs"] {
        w.write(path, "pub fn build() {}\n");
    }
    w.write("src/parent/mod.rs", PARENT);
    w.write("src/parent/child.rs", "use super::{writer as emit, Workspace as Room};\nuse super::*;\nuse crate::provider::{self as direct};\nmod grandchild;\nfn probe() {\n    emit();\n    inherited();\n    Room::new();\n    direct::build();\n}\n");
    w.write(
        "src/parent/child/grandchild.rs",
        "use super::*;\nfn probe() {\n    inherited();\n    writer();\n}\n",
    );
    for (path, source) in [
        ("src/lexical_direct.rs", "fn probe() { use crate::provider::*;\n build();\n }\n"),
        ("src/lexical_typed.rs", "fn probe() { use crate::glob_two::*;\n let value = Workspace::new();\n value.work();\n }\n"),
        ("src/lexical_transitive.rs", "fn probe() { use crate::glob_two::*;\n build();\n }\n"),
        ("src/lexical_block.rs", "fn probe() { { use crate::glob_two::*;\n build();\n } }\n"),
        ("src/lexical_named.rs", "fn probe() { use crate::glob_two::*; use crate::provider::other as build;\n build();\n }\n"),
        ("src/lexical_broken.rs", "fn probe() { use crate::glob_two::*; use crate::provider::missing as build;\n build();\n }\n"),
        ("src/lexical_own.rs", "fn probe() { use crate::glob_two::*; fn build() {}\n build();\n }\n"),
        ("src/lexical_private.rs", "fn probe() { use crate::provider::*;\n hidden();\n }\n"),
        ("src/lexical_ambiguous.rs", "fn probe() { use crate::provider::*; use crate::another::*;\n build();\n }\n"),
        ("src/lexical_incomplete.rs", "fn probe() { use crate::provider::*; use crate::glob_missing::*;\n build();\n }\n"),
        ("src/module_facade/mod.rs", "pub mod nested;\nmod private_nested;\n"),
        ("src/module_facade/nested.rs", "pub fn build() {}\n"),
        ("src/module_facade/private_nested.rs", "pub fn build() {}\n"),
        ("src/module_outer.rs", "pub use crate::module_facade::*;\n"),
        ("src/module_direct.rs", "use crate::module_facade::*;\nfn probe() {\n nested::build();\n private_nested::build();\n}\n"),
        ("src/module_forwarded.rs", "use crate::module_outer::*;\nfn probe() {\n nested::build();\n private_nested::build();\n}\n"),
        ("src/another.rs", "pub fn build() {} pub fn private_only() {}\n"),
        ("src/glob_one.rs", GLOB_ONE),
        ("src/glob_two.rs", "pub use crate::{glob_one::*, provider::{Workspace as Room}};\n"),
        ("src/glob_ambiguous.rs", "pub use crate::{provider::*, another::*};\n"),
        ("src/glob_same.rs", "pub use crate::{glob_one::*, glob_two::*};\n"),
        ("src/glob_mixed.rs", "pub use crate::provider::*; use crate::another::*;\n"),
        ("src/glob_outer.rs", "pub use crate::glob_mixed::*;\n"),
        ("src/glob_cycle.rs", "pub use crate::{provider::*, glob_cycle::*};\n"),
        ("src/glob_missing.rs", "pub use crate::{provider::*, missing::*};\n"),
        ("src/glob_consumer.rs", "use crate::glob_two::*;\nfn probe(value: Workspace) {\n    build();\n    Workspace::new();\n    value.work();\n    hidden();\n}\n"),
        ("src/glob_shadow.rs", "use crate::glob_two::*; fn build() {} fn probe() { build(); }\n"),
        ("src/glob_named.rs", "use crate::glob_two::*; use crate::provider::other as build; fn probe() { build(); }\n"),
        ("src/glob_broken.rs", "use crate::glob_two::*; use crate::provider::missing as build; fn probe() { build(); }\n"),
        ("src/glob_negative.rs", "use crate::glob_ambiguous::*;\nuse crate::glob_ambiguous as ambiguous;\nuse crate::glob_same as same;\nuse crate::glob_mixed as mixed;\nuse crate::glob_outer as outer;\nuse crate::glob_cycle as cyclic;\nuse crate::glob_missing as missing;\nfn probe() {\n    build();\n    ambiguous::build();\n    same::build();\n    mixed::build();\n    mixed::other();\n    mixed::private_only();\n    outer::build();\n    outer::other();\n    cyclic::build();\n    missing::build();\n}\n"),
    ] { w.write(path, source); }
    w.write("src/facade.rs", FACADE);
    w.write(
        "src/bridge.rs",
        &format!("pub use crate::facade::{{forwarded as twice, Room as Hall, tools}};\n{}pub use crate::provider::build as depth64;\n", (0..64).map(|depth| format!("pub use self::depth{} as depth{depth};\n", depth + 1)).collect::<String>()),
    );
    w.write("src/consumer.rs", "use crate::bridge::{twice as invoke, Hall, tools};\nuse super::facade::{self as api, absent, conflict, leaked, partial, cyclic, same, hidden_mix, deep_mix};\nuse crate::cycle_a::ring;\nuse crate::parent::writer as private_parent;\nfn probe() {\n    invoke();\n    Hall::new();\n    tools::build();\n    api::forwarded();\n    absent();\n    conflict();\n    ring();\n    private_parent();\n    leaked();\n    partial();\n    api::partial();\n    cyclic();\n    api::cyclic();\n    same();\n    api::same();\n    hidden_mix();\n    api::hidden_mix();\n    deep_mix();\n    api::deep_mix();\n}\n");
    w.write("src/cycle_a.rs", "pub use crate::cycle_b::ring;\n");
    w.write("src/cycle_b.rs", "pub use crate::cycle_a::ring;\n");
    w.write("src/a.rs", "use crate::provider::build as hidden;\n");
    w.write(
        "src/b/mod.rs",
        "use crate::a::hidden as indirect;\nmod child;\n",
    );
    w.write(
        "src/b/child.rs",
        "use super::indirect;\nfn probe() {\n    indirect();\n}\n",
    );
    w.build();
    w
}

pub(super) struct Observation {
    pub statuses: Vec<String>,
    pub targets: Vec<String>,
    pub canonical: Vec<String>,
    pub dependencies: Vec<String>,
    pub expected_dependencies: Vec<String>,
}

pub(super) fn observe(
    conn: &Connection,
    &(owner, expression, target, _): &Probe<'_>,
) -> Observation {
    let coverage: Vec<(usize, String)> = conn.prepare("SELECT c.line,c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.line,c.status").unwrap()
        .query_map(params![owner, expression], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    let mut targets = Vec::<String>::new();
    for (line, _) in &coverage {
        targets.extend(conn.prepare("SELECT n.id FROM edge_occurrences o JOIN nodes n ON n.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path=?1 AND o.line=?2 AND o.kind='calls' ORDER BY n.id").unwrap()
            .query_map(params![owner, line], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<String>>>().unwrap());
    }
    targets.sort();
    targets.dedup();
    let canonical = target.map(|(path, name)| conn.prepare("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 AND n.kind IN ('function','method') ORDER BY n.id").unwrap()
        .query_map(params![path, name], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<String>>>().unwrap()).unwrap_or_default();
    let dependencies = conn.prepare("SELECT DISTINCT f.path FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE p.path=?1 AND d.symbol=?2 AND d.kind='calls' AND d.resolution='resolved' ORDER BY f.path").unwrap()
        .query_map(params![owner, expression], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    let expected_dependencies = target
        .filter(|(path, _)| *path != owner)
        .map(|(path, _)| path.to_owned())
        .into_iter()
        .collect();
    Observation {
        statuses: coverage.into_iter().map(|(_, status)| status).collect(),
        targets,
        canonical,
        dependencies,
        expected_dependencies,
    }
}

pub(super) fn snapshot(conn: &Connection) -> Vec<Vec<String>> {
    [
            "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
            "SELECT json_array(s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence) FROM edge_occurrences o JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id JOIN coverage_evidence e ON e.evidence_id=o.evidence_id JOIN coverage_evidence c ON c.evidence_id=o.confidence_id ORDER BY s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence",
            "SELECT json_array(p.path,c.line,x.expression,c.status,e.evidence) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id ORDER BY p.path,c.line,x.expression,c.status,e.evidence",
            "SELECT json_array(p.path,d.ordinal,f.path,d.kind,d.symbol,d.resolution) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id LEFT JOIN files f ON f.path_hash=d.target_hash ORDER BY p.path,d.ordinal",
            "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
        ].iter().map(|sql| conn.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()).collect()
}

pub(super) fn fresh_cold(w: &PersistedWorkspaceHarness) -> Connection {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = w.path(format!(".forge/rust-cold-{id}.sqlite"));
    w.build_to(&path);
    w.open_at(&path)
}

pub(super) fn errors(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM errors", [], |r| r.get(0))
        .unwrap()
}

fn exact<'a>(owner: &'a str, expression: &'a str, path: &'a str, name: &'a str) -> Probe<'a> {
    (owner, expression, Some((path, name)), "resolved")
}

fn unresolved<'a>(owner: &'a str, expression: &'a str) -> Probe<'a> {
    (owner, expression, None, "unresolved")
}

fn ambiguous<'a>(owner: &'a str, expression: &'a str) -> Probe<'a> {
    (owner, expression, None, "ambiguous")
}

pub(super) fn initial_probes() -> Vec<Probe<'static>> {
    vec![
        exact(
            "src/lexical_typed.rs",
            "Workspace::new",
            "src/provider.rs",
            "new",
        ),
        exact(
            "src/lexical_typed.rs",
            "value.work",
            "src/provider.rs",
            "work",
        ),
        exact("src/lexical_direct.rs", "build", "src/provider.rs", "build"),
        exact(
            "src/lexical_transitive.rs",
            "build",
            "src/provider.rs",
            "build",
        ),
        exact("src/lexical_block.rs", "build", "src/provider.rs", "build"),
        exact("src/lexical_named.rs", "build", "src/provider.rs", "other"),
        unresolved("src/lexical_broken.rs", "build"),
        exact("src/lexical_own.rs", "build", "src/lexical_own.rs", "build"),
        unresolved("src/lexical_private.rs", "hidden"),
        ambiguous("src/lexical_ambiguous.rs", "build"),
        unresolved("src/lexical_incomplete.rs", "build"),
        exact(
            "src/module_direct.rs",
            "nested::build",
            "src/module_facade/nested.rs",
            "build",
        ),
        exact(
            "src/module_forwarded.rs",
            "nested::build",
            "src/module_facade/nested.rs",
            "build",
        ),
        unresolved("src/module_direct.rs", "private_nested::build"),
        unresolved("src/module_forwarded.rs", "private_nested::build"),
        exact(
            "src/parent/child/grandchild.rs",
            "inherited",
            "src/provider.rs",
            "build",
        ),
        exact(
            "src/parent/child/grandchild.rs",
            "writer",
            "src/provider.rs",
            "build",
        ),
        exact("src/glob_consumer.rs", "build", "src/provider.rs", "build"),
        exact(
            "src/glob_consumer.rs",
            "Workspace::new",
            "src/provider.rs",
            "new",
        ),
        exact(
            "src/glob_consumer.rs",
            "value.work",
            "src/provider.rs",
            "work",
        ),
        unresolved("src/glob_consumer.rs", "hidden"),
        exact("src/glob_shadow.rs", "build", "src/glob_shadow.rs", "build"),
        exact("src/glob_named.rs", "build", "src/provider.rs", "other"),
        unresolved("src/glob_broken.rs", "build"),
        ambiguous("src/glob_negative.rs", "build"),
        unresolved("src/glob_negative.rs", "ambiguous::build"),
        exact(
            "src/glob_negative.rs",
            "same::build",
            "src/provider.rs",
            "build",
        ),
        unresolved("src/glob_negative.rs", "mixed::build"),
        exact(
            "src/glob_negative.rs",
            "mixed::other",
            "src/provider.rs",
            "other",
        ),
        unresolved("src/glob_negative.rs", "mixed::private_only"),
        unresolved("src/glob_negative.rs", "outer::build"),
        exact(
            "src/glob_negative.rs",
            "outer::other",
            "src/provider.rs",
            "other",
        ),
        unresolved("src/glob_negative.rs", "cyclic::build"),
        unresolved("src/glob_negative.rs", "missing::build"),
        exact("src/lib.rs", "provider::build", "src/provider.rs", "build"),
        unresolved("src/lib.rs", "undeclared::build"),
        unresolved("src/lib.rs", "attributed::build"),
        unresolved("src/lib.rs", "disabled::build"),
        exact(
            "src/parent/child.rs",
            "inherited",
            "src/provider.rs",
            "build",
        ),
        exact("src/parent/child.rs", "emit", "src/provider.rs", "build"),
        exact("src/parent/child.rs", "Room::new", "src/provider.rs", "new"),
        exact(
            "src/parent/child.rs",
            "direct::build",
            "src/provider.rs",
            "build",
        ),
        exact("src/consumer.rs", "invoke", "src/provider.rs", "build"),
        exact("src/consumer.rs", "Hall::new", "src/provider.rs", "new"),
        exact(
            "src/consumer.rs",
            "tools::build",
            "src/provider.rs",
            "build",
        ),
        exact(
            "src/consumer.rs",
            "api::forwarded",
            "src/provider.rs",
            "build",
        ),
        unresolved("src/consumer.rs", "absent"),
        ambiguous("src/consumer.rs", "conflict"),
        unresolved("src/consumer.rs", "ring"),
        unresolved("src/consumer.rs", "private_parent"),
        unresolved("src/consumer.rs", "leaked"),
        unresolved("src/b/child.rs", "indirect"),
        unresolved("src/consumer.rs", "partial"),
        unresolved("src/consumer.rs", "api::partial"),
        unresolved("src/consumer.rs", "cyclic"),
        unresolved("src/consumer.rs", "api::cyclic"),
        unresolved("src/consumer.rs", "hidden_mix"),
        unresolved("src/consumer.rs", "api::hidden_mix"),
        unresolved("src/consumer.rs", "deep_mix"),
        unresolved("src/consumer.rs", "api::deep_mix"),
        exact("src/consumer.rs", "same", "src/provider.rs", "build"),
        exact("src/consumer.rs", "api::same", "src/provider.rs", "build"),
    ]
}

#[test]
fn named_glob_lexical_and_module_forwarding_have_exact_persisted_targets() {
    let w = forwarding_workspace();
    let conn = w.open();
    commitments::verify(&conn).unwrap();
    assert_eq!(errors(&conn), 0);
    for probe in initial_probes() {
        let actual = observe(&conn, &probe);
        assert!(!actual.statuses.is_empty(), "{probe:?}");
        assert!(
            actual.statuses.iter().all(|status| status == probe.3),
            "{probe:?}: {:?}",
            actual.statuses
        );
        assert_eq!(
            actual.canonical.len(),
            usize::from(probe.2.is_some()),
            "{probe:?}"
        );
        assert_eq!(actual.targets, actual.canonical, "{probe:?}");
        assert_eq!(
            actual.dependencies, actual.expected_dependencies,
            "{probe:?}"
        );
    }
    let cold = fresh_cold(&w);
    commitments::verify(&cold).unwrap();
    let _ = cold.execute_batch("COMMIT");
    assert_eq!(snapshot(&conn), snapshot(&cold));
}
