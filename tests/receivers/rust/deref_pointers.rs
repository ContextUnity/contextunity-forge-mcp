#[test]
fn rust_smart_pointer_deref_dispatch_persists_exact_providers_and_boundaries() {
    use crate::common::Workspace;

    let workspace = Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"smart-pointer-provenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let sources = [
        (
            "lib.rs",
            "mod service; mod box_case; mod arc_case; mod rc_case; mod option_case; mod unimported_case; mod shadow_case; mod one; mod two; mod ambiguous_case; mod custom_case; mod custom_shadow; mod duplicate_case; mod cycle_case; mod attributed_case; mod bounded_case; mod std_deref_case; mod core_deref_case; mod imported_local_case; mod fake_roots; mod glob_root_case; mod parent_root;",
        ),
        (
            "service.rs",
            "pub struct Item; impl Item { pub fn use_item(&self) {} } pub struct Service { pub item: Item } impl Service { pub fn work(&self) {} pub fn as_ref(&self) {} pub fn new() -> Self { unknown() } }",
        ),
        (
            "box_case.rs",
            "use crate::service::Service; pub fn run(value: Box<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "arc_case.rs",
            "use crate::service::Service; use std::sync::Arc; pub fn run(value: Arc<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "rc_case.rs",
            "use crate::service::Service; use std::rc::Rc; pub fn run(value: Rc<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "option_case.rs",
            "use crate::service::Service; pub fn run(value: Option<Service>) { value.work(); }",
        ),
        (
            "unimported_case.rs",
            "use crate::service::Service; pub fn run(value: Arc<Service>) { value.work(); }",
        ),
        (
            "shadow_case.rs",
            "use crate::service::Service; pub struct Arc<T>(T); impl<T> Arc<T> { pub fn as_ref(&self) -> &T { &self.0 } } pub fn run(value: Arc<Service>) { value.work(); value.as_ref(); }",
        ),
        ("one.rs", "pub struct Service; impl Service { pub fn work(&self) {} }"),
        ("two.rs", "pub struct Service; impl Service { pub fn work(&self) {} }"),
        (
            "ambiguous_case.rs",
            "use crate::one::Service; use crate::two::Service; pub fn run(value: Box<Service>) { value.work(); }",
        ),
        (
            "custom_case.rs",
            "use crate::service::Service; pub struct Wrapper; impl std::ops::Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub struct Other; impl Other { pub fn use_item(&self) {} } pub struct OwnWrapper { pub item: Other } impl OwnWrapper { pub fn work(&self) {} } impl core::ops::Deref for OwnWrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper, own: &OwnWrapper) { value.work(); value.item.use_item(); Wrapper::new(); own.work(); own.item.use_item(); }",
        ),
        (
            "custom_shadow.rs",
            "use crate::service::Service; trait Deref { type Target; } pub struct Local; impl Deref for Local { type Target = Service; } mod std { pub mod ops { pub trait Deref { type Target; } } } pub struct Shadow; impl std::ops::Deref for Shadow { type Target = Service; } pub struct Absolute; impl ::std::ops::Deref for Absolute { type Target = Service; } pub fn run(local: &Local, shadow: &Shadow, absolute: &Absolute) { local.work(); shadow.work(); absolute.work(); }",
        ),
        (
            "duplicate_case.rs",
            "use crate::service::Service; pub struct Wrapper; impl std::ops::Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } impl core::ops::Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper) { value.work(); value.item.use_item(); }",
        ),
        (
            "cycle_case.rs",
            "pub struct First; pub struct Second; impl Second { pub fn work(&self) {} } impl std::ops::Deref for First { type Target = Second; fn deref(&self) -> &Self::Target { unknown() } } impl std::ops::Deref for Second { type Target = First; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &First) { value.work(); }",
        ),
        (
            "attributed_case.rs",
            "use crate::service::Service; pub struct Wrapper; #[cfg(any())] impl std::ops::Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper) { value.work(); }",
        ),
        (
            "bounded_case.rs",
            "use crate::service::Service; pub struct Wrapper; impl Wrapper { pub fn work<T: Clone>(&self) {} } impl std::ops::Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub trait Work { fn work<T: Clone>(&self); } pub struct TraitWrapper; impl Work for TraitWrapper { fn work<T: Clone>(&self) {} } impl std::ops::Deref for TraitWrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper, bounded_trait: &TraitWrapper) { value.work(); bounded_trait.work(); }",
        ),
        (
            "std_deref_case.rs",
            "use crate::service::Service; use std::ops::Deref; pub struct Wrapper; impl Deref for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper) { value.work(); }",
        ),
        (
            "core_deref_case.rs",
            "use crate::service::Service; use core::ops::Deref as Forward; pub struct Wrapper; impl Forward for Wrapper { type Target = Service; fn deref(&self) -> &Self::Target { unknown() } } pub fn run(value: &Wrapper) { value.work(); }",
        ),
        (
            "imported_local_case.rs",
            "use crate::service::Service; use crate::custom_shadow::Deref as Forward; pub struct Wrapper; impl Forward for Wrapper { type Target = Service; } pub fn run(value: &Wrapper) { value.work(); }",
        ),
        (
            "glob_root_case.rs",
            "use crate::service::Service; use crate::fake_roots::*; use std::ops::Deref; pub struct Wrapper; impl std::ops::Deref for Wrapper { type Target = Service; } pub struct Bare; impl Deref for Bare { type Target = Service; } pub fn run(value: &Wrapper, bare: &Bare) { value.work(); bare.work(); }",
        ),
        (
            "fake_roots.rs",
            "pub mod std { pub mod ops { pub trait Deref { type Target; } } }",
        ),
        (
            "parent_root/mod.rs",
            "pub mod core { pub mod ops { pub trait Deref { type Target; } } } mod child;",
        ),
        (
            "parent_root/child.rs",
            "use super::*; use crate::service::Service; pub struct Wrapper; impl core::ops::Deref for Wrapper { type Target = Service; } pub fn run(value: &Wrapper) { value.work(); }",
        ),
    ];
    for (name, source) in sources {
        workspace.write(format!("src/{name}"), source);
    }

    workspace.build();
    let conn = workspace.open();
    let mut coverage = conn
        .prepare("SELECT c.status,v.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.status,v.evidence")
        .unwrap();
    let mut edges = conn
        .prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.kind='calls' AND v.evidence=?2 ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap();

    for (path, expression, statuses, target, origin) in [
        (
            "src/custom_shadow.rs",
            "absolute.work",
            &["resolved"][..],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/glob_root_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/glob_root_case.rs",
            "bare.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/parent_root/child.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/std_deref_case.rs",
            "value.work",
            &["resolved"][..],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/core_deref_case.rs",
            "value.work",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/imported_local_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/bounded_case.rs",
            "value.work",
            &["unresolved"][..],
            None,
            None,
        ),
        (
            "src/bounded_case.rs",
            "bounded_trait.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/custom_case.rs",
            "value.work",
            &["resolved"][..],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/custom_case.rs",
            "value.item.use_item",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/custom_case.rs",
            "own.work",
            &["resolved"],
            Some("src/custom_case.rs"),
            None,
        ),
        (
            "src/custom_case.rs",
            "own.item.use_item",
            &["resolved"],
            Some("src/custom_case.rs"),
            None,
        ),
        (
            "src/custom_case.rs",
            "Wrapper::new",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/custom_shadow.rs",
            "local.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/custom_shadow.rs",
            "shadow.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/duplicate_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/duplicate_case.rs",
            "value.item.use_item",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/cycle_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/attributed_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/box_case.rs",
            "value.work",
            &["resolved"][..],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/arc_case.rs",
            "value.work",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/rc_case.rs",
            "value.work",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/box_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/arc_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/rc_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/option_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/unimported_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/shadow_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/shadow_case.rs",
            "value.as_ref",
            &["resolved"],
            Some("src/shadow_case.rs"),
            None,
        ),
        (
            "src/ambiguous_case.rs",
            "value.work",
            &["unresolved", "ambiguous"],
            None,
            None,
        ),
    ] {
        let rows: Vec<(String, String)> = coverage
            .query_map([path, expression], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.len(), 1, "{path} {expression}: {rows:?}");
        assert!(
            statuses.contains(&rows[0].0.as_str()),
            "{path} {expression}: {rows:?}"
        );
        if let Some(origin) = origin {
            assert!(rows[0].1.contains(origin), "{path} {expression}: {rows:?}");
        }
        let targets: Vec<String> = edges
            .query_map([path, expression], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            targets,
            target.map_or_else(Vec::new, |target| vec![target.to_owned()]),
            "{path} {expression}: {rows:?}"
        );
    }
}
