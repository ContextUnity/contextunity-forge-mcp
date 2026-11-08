use super::*;
use crate::languages::harness::PersistedSnapshot;
use crate::languages::PersistedWorkspaceHarness;

const MANIFEST: &str = ".forge/frameworks/property-proof.toml";

struct FactoryWorkspace {
    workspace: PersistedWorkspaceHarness,
    manifest_source: String,
    disabled_source: String,
}

impl FactoryWorkspace {
    fn new() -> Self {
        let workspace = crate::languages::PersistedWorkspaceHarness::new();
        for (path, source) in [
            (
                "factory.ts",
                "function run() { const button = document.createElement('button'); button.addEventListener('click', () => {}); const alias = button; alias.addEventListener('click', () => {}); const root = document.querySelector('#root'); if (root) root.querySelector('.child'); const found = document.getElementById('root'); if (found) found.getAttribute('id'); button.unknownMethod(); }",
            ),
            (
                "shadow.ts",
                "function run(document) { const button = document.createElement('button'); button.addEventListener('click', () => {}); }",
            ),
            (
                "reassigned_document.ts",
                "function run() { document = {}; const button = document.createElement('button'); button.addEventListener('click', () => {}); }",
            ),
            (
                "reassigned_result.ts",
                "function run() { let button = document.createElement('button'); button = {}; button.addEventListener('click', () => {}); }",
            ),
            (
                "shadow.js",
                "function run(document) { document.addEventListener('click', event => event.preventDefault()); }",
            ),
            (
                "unknown_factory.ts",
                "function run() { const button = document.makeElement('button'); button.addEventListener('click', () => {}); }",
            ),
            (
                "untyped.ts",
                "function run() { const button = {}; button.addEventListener('click', () => {}); }",
            ),
            (
                "local.ts",
                "class Element { addEventListener() {} } function run() { const button = new Element(); button.addEventListener('click'); }",
            ),
            (
                "literal.ts",
                "function run() { const input = document.createElement('input'); input.value.trim(); const script = document.createElement('script'); script.src.trim(); document.createElement('input').value.trim(); document.createElement('script').src.trim(); input.src.trim(); script.value.trim(); input.invented();\ninput.value(); }",
            ),
            (
                "literal.js",
                "function run() { const input = document.createElement('input'); input.value.trim(); const script = document.createElement('script'); script.src.trim(); document.createElement('input').value.trim(); document.createElement('script').src.trim(); input.src.trim(); script.value.trim(); input.invented();\ninput.value(); }",
            ),
            (
                "literal_shadow.ts",
                "function run(document) { const input = document.createElement('input'); input.value.trim(); document.createElement('script').src.trim(); }",
            ),
            (
                "literal_write.ts",
                "function run() { document = {}; const input = document.createElement('input'); input.value.trim(); }",
            ),
            (
                "literal_unknown.ts",
                "function run(tag) { const dynamic = document.createElement(tag); dynamic.addEventListener('click', () => {}); dynamic.value.trim(); const missing = document.createElement(); missing.addEventListener('click', () => {}); const spread = document.createElement(...['input']); spread.addEventListener('click', () => {}); spread.value.trim(); const excess = document.createElement('input', 1, 2, 3, 4); excess.addEventListener('click', () => {}); excess.value.trim(); const unknown = document.createElement('unmapped'); unknown.addEventListener('click', () => {}); unknown.value.trim(); }",
            ),
        ] {
            workspace.write(path, source);
        }
        for extension in ["ts", "js"] {
            workspace.write(
                format!("property.{extension}"),
                r#"function read() {
const input = document.createElement('input');
const script = document.createElement('script');
const a = input.value;
const b = script.src;
const c = document.createElement('input').value;
const d = document.createElement('script').src;
a.trim(); b.trim(); c.trim(); d.trim();
const wrong = document.createElement('input').src;
const other = document.createElement('script').value;
const dynamic = document.createElement(tag).value;
const spread = document.createElement(...['input']).value;
const fifth = document.createElement('input', 1, 2, 3, 4).value;
const unknown = document.createElement('unmapped').value;
const escaped = document.createElement('in\put').value;
wrong.trim(); other.trim(); dynamic.trim(); spread.trim(); fifth.trim(); unknown.trim(); escaped.trim();
}"#,
            );
            workspace.write(
                format!("property_call.{extension}"),
                "function read() { const input = document.createElement('input'); input.value(); document.createElement('input').value(); document.createElement('script').src(); }",
            );
            workspace.write(
                format!("property_alias.{extension}"),
                "function read() { const v = document.createElement('input').value; const s = document.createElement('script').src; const alias = v; v(); s(); alias(); }",
            );
            workspace.write(format!("property_shadow.{extension}"), "function read(document) { const input = document.createElement('input'); const value = input.value; const direct = document.createElement('input').value; value.trim(); direct.trim(); }");
            workspace.write(
                format!("property_write.{extension}"),
                "function read() { document = {}; const value = document.createElement('input').value; value.trim(); }",
            );
            workspace.write(format!("property_local.{extension}"), "class LocalElement { value = 'local'; } function make() { return new LocalElement(); } function read() { const item = make(); const a = item.value; const b = make().value; }\nfunction call() { const item = make(); item.value(); }");
        }
        let mut rules = Vec::new();
        for language in ["javascript", "typescript"] {
            for (id, global, method, literal, name, property) in [
                (
                    "original",
                    "nodeFactory",
                    "make",
                    "field",
                    "PropertyProof",
                    "payload",
                ),
                (
                    "renamed",
                    "viewFactory",
                    "forge",
                    "cell",
                    "RenamedProof",
                    "content",
                ),
            ] {
                rules.push(serde_json::json!({
                    "id":format!("{id}-{language}"), "language":language,
                    "operation":{"kind":"receiver_type", "receiver":{
                        "name":name, "members":[], "properties":BTreeMap::from([(property,"string")]),
                        "callback_callees":[], "generic":false,
                        "argument_factories":[{"callee":{"kind":"global_member", "receiver":global, "member":method}, "argument":0, "literals":[literal]}]
                    }}
                }));
                let extension = if language == "javascript" { "js" } else { "ts" };
                workspace.write(format!("{id}.{extension}"), &format!(
                "function read() {{ const item = {global}.{method}('{literal}'); const a = item.{property}; const b = {global}.{method}('{literal}').{property}; a.trim(); b.trim(); }}\nfunction call() {{ {global}.{method}('{literal}').{property}(); }}"
            ));
            }
        }
        let pack = serde_json::json!({"syntax_rules":rules});
        let manifest_source = toml::to_string(&pack).unwrap();
        workspace.write(MANIFEST, &manifest_source);

        let mut disabled = pack.clone();
        for rule in disabled["syntax_rules"].as_array_mut().unwrap() {
            rule["operation"]["receiver"]["argument_factories"] = serde_json::json!([]);
        }
        let disabled_source = toml::to_string(&disabled).unwrap();
        workspace.build();
        Self {
            workspace,
            manifest_source,
            disabled_source,
        }
    }

    fn disable(&self) {
        self.workspace.write(MANIFEST, &self.disabled_source);
        self.workspace.delta(&[MANIFEST]);
    }

    fn restore(&self) {
        self.workspace.write(MANIFEST, &self.manifest_source);
        self.workspace.delta(&[MANIFEST]);
    }
}

fn baseline() -> &'static PersistedSnapshot {
    static BASELINE: std::sync::OnceLock<PersistedSnapshot> = std::sync::OnceLock::new();
    BASELINE.get_or_init(|| {
        let fixture = FactoryWorkspace::new();
        PersistedSnapshot::from_connection(&fixture.workspace.open())
    })
}

fn configured_sources() -> Vec<(String, String, String, String, String)> {
    let mut sources = Vec::new();
    for extension in ["ts", "js"] {
        for (id, global, method, literal, property) in [
            ("original", "nodeFactory", "make", "field", "payload"),
            ("renamed", "viewFactory", "forge", "cell", "content"),
        ] {
            sources.push((
                format!("{id}.{extension}"),
                format!("item.{property}"),
                format!("{global}.{method}('{literal}').{property}"),
                id.into(),
                format!("{global}.{method}"),
            ));
        }
    }
    sources
}

#[test]
fn factory_methods_and_aliases_require_verified_receiver_origin() {
    let conn = baseline().open();
    for (path, expression) in [
        ("factory.ts", "button.addEventListener"),
        ("factory.ts", "alias.addEventListener"),
        ("factory.ts", "root.querySelector"),
        ("factory.ts", "found.getAttribute"),
    ] {
        let row = persisted_coverage(&conn, path, None, expression);
        assert_eq!(row.0, "external");
        assert!(
            row.1.contains("builtin:web_api"),
            "{path}:{expression} {row:?}"
        );
    }
    for (path, expression) in [
        ("factory.ts", "button.unknownMethod"),
        ("shadow.ts", "button.addEventListener"),
        ("reassigned_document.ts", "button.addEventListener"),
        ("reassigned_result.ts", "button.addEventListener"),
        ("unknown_factory.ts", "button.addEventListener"),
        ("untyped.ts", "button.addEventListener"),
        ("shadow.js", "document.addEventListener"),
        ("shadow.js", "event.preventDefault"),
    ] {
        let row = persisted_coverage(&conn, path, None, expression);
        assert_eq!(row.0, "unresolved");
    }
    assert_eq!(
        persisted_coverage(&conn, "local.ts", None, "button.addEventListener").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='local.ts' AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='local.ts' AND dst.kind='method' AND dst.name='addEventListener'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(local_edge, 1);
}

#[test]
fn literal_factory_members_have_finite_element_types() {
    let conn = baseline().open();
    for path in ["literal.ts", "literal.js"] {
        for (expression, expected) in [
            ("input.value.trim", "external"),
            ("script.src.trim", "external"),
            ("document.createElement('input').value.trim", "external"),
            ("document.createElement('script').src.trim", "external"),
            ("input.src.trim", "unresolved"),
            ("script.value.trim", "unresolved"),
            ("input.invented", "unresolved"),
        ] {
            let row = persisted_coverage(&conn, path, None, expression);
            assert_eq!(row.0, expected);
        }
    }
}

#[test]
fn literal_properties_are_not_callable() {
    let conn = baseline().open();
    for path in ["literal.ts", "literal.js"] {
        let status: String = conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='input.value'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(status, "unresolved");
    }
}

#[test]
fn property_reads_preserve_literal_factory_provenance() {
    let conn = baseline().open();
    for extension in ["ts", "js"] {
        let path = format!("property.{extension}");
        for expression in [
            "input.value",
            "script.src",
            "document.createElement('input').value",
            "document.createElement('script').src",
            "a.trim",
            "b.trim",
            "c.trim",
            "d.trim",
        ] {
            let row = persisted_coverage(&conn, &path, None, expression);
            assert_eq!(row.0, "external");
            if !expression.ends_with(".trim") {
                let language = if extension == "ts" {
                    "typescript"
                } else {
                    "javascript"
                };
                for token in [
                    format!("finite {language} configured member"),
                    "pack web".into(),
                    "receiver rule literal-".into(),
                    "receiver origin external import document.createElement".into(),
                ] {
                    assert!(
                        row.1.contains(&token),
                        "property provenance {path}:{expression}: missing {token}: {row:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn unknown_or_mismatched_factory_arguments_do_not_prove_property_reads() {
    let conn = baseline().open();
    for extension in ["ts", "js"] {
        let path = format!("property.{extension}");
        for expression in [
            "document.createElement('input').src",
            "document.createElement('script').value",
            "document.createElement(tag).value",
            "document.createElement(...['input']).value",
            "document.createElement('input', 1, 2, 3, 4).value",
            "document.createElement('unmapped').value",
            "document.createElement('in\\put').value",
            "wrong.trim",
            "other.trim",
            "dynamic.trim",
            "spread.trim",
            "fifth.trim",
            "unknown.trim",
            "escaped.trim",
        ] {
            let row = persisted_coverage(&conn, &path, None, expression);
            assert_eq!(row.0, "unresolved");
        }
    }
}

#[test]
fn property_calls_aliases_and_shadowed_factories_remain_unresolved() {
    let conn = baseline().open();
    for extension in ["ts", "js"] {
        for (prefix, expressions) in [
            ("property_alias", vec!["v", "s", "alias"]),
            (
                "property_call",
                vec![
                    "input.value",
                    "document.createElement('input').value",
                    "document.createElement('script').src",
                ],
            ),
            (
                "property_shadow",
                vec![
                    "document.createElement('input').value",
                    "value.trim",
                    "direct.trim",
                ],
            ),
            (
                "property_write",
                vec!["document.createElement('input').value", "value.trim"],
            ),
        ] {
            for expression in expressions {
                let row =
                    persisted_coverage(&conn, &format!("{prefix}.{extension}"), None, expression);
                assert_eq!(row.0, "unresolved");
            }
        }
    }
}

#[test]
fn shadowed_plain_reads_emit_no_observation_or_success_edges() {
    let conn = baseline().open();
    for extension in ["ts", "js"] {
        let shadow_path = format!("property_shadow.{extension}");
        let absent: i64 = conn.query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='input.value'",
            [&shadow_path], |row| row.get(0),
        ).unwrap();
        assert_eq!(absent, 0);
        let edges: i64 = conn
            .query_row("SELECT count(*) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=owner_id)=?1 AND kind IN ('calls','references')", [&shadow_path], |row| row.get(0))
            .unwrap();
        assert_eq!(edges, 0);
    }
}

#[test]
fn local_properties_keep_exact_field_targets_and_stay_noncallable() {
    let conn = baseline().open();
    for extension in ["ts", "js"] {
        let local_path = format!("property_local.{extension}");
        for expression in ["item.value", "make().value"] {
            let status: String = conn.query_row(
                "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
                rusqlite::params![local_path,expression], |row| row.get(0),
            ).unwrap();
            assert_eq!(status, "resolved");
        }
        let local_targets: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?1 AND dst.kind='field' AND dst.name='value'",
            [&local_path], |row| row.get(0),
        ).unwrap();
        assert_eq!(local_targets, 2);
        let local_call: String = conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='item.value'",
            [&local_path], |row| row.get(0),
        ).unwrap();
        assert_eq!(local_call, "unresolved");
    }
}

#[test]
fn literal_argument_boundaries_keep_generic_methods_without_narrow_properties() {
    let conn = baseline().open();
    for (path, expression, expected) in [
        ("literal_shadow.ts", "input.value.trim", "unresolved"),
        (
            "literal_shadow.ts",
            "document.createElement('script').src.trim",
            "unresolved",
        ),
        ("literal_write.ts", "input.value.trim", "unresolved"),
        ("literal_unknown.ts", "dynamic.addEventListener", "external"),
        ("literal_unknown.ts", "dynamic.value.trim", "unresolved"),
        ("literal_unknown.ts", "missing.addEventListener", "external"),
        ("literal_unknown.ts", "spread.addEventListener", "external"),
        ("literal_unknown.ts", "spread.value.trim", "unresolved"),
        ("literal_unknown.ts", "excess.addEventListener", "external"),
        ("literal_unknown.ts", "excess.value.trim", "unresolved"),
        ("literal_unknown.ts", "unknown.addEventListener", "external"),
        ("literal_unknown.ts", "unknown.value.trim", "unresolved"),
    ] {
        let row = persisted_coverage(&conn, path, None, expression);
        assert_eq!(row.0, expected);
    }
}

#[test]
fn configured_property_mapping_and_provider_evidence_follow_disable_and_restore() {
    let fixture = FactoryWorkspace::new();
    for (phase, expected) in [(0, "external"), (1, "unresolved"), (2, "external")] {
        match phase {
            0 => (),
            1 => fixture.disable(),
            _ => fixture.restore(),
        }
        let conn = fixture.workspace.open();
        for (path, ordinary, direct, id, origin) in configured_sources() {
            for expression in [&direct, "a.trim", "b.trim"] {
                let (status, evidence) = persisted_coverage(&conn, &path, Some(1), expression);
                assert_eq!(status, expected, "{path}: {expression}: {evidence}");
                if expected == "external" && expression == direct {
                    assert!(evidence.contains("pack property-proof"));
                    assert!(evidence.contains(&format!("receiver rule {id}-")));
                    assert!(evidence.contains(&format!("receiver origin external import {origin}")));
                }
            }
            if expected == "external" {
                let (status, evidence) = persisted_coverage(&conn, &path, Some(1), &ordinary);
                assert_eq!(status, "external");
                assert!(evidence.contains("pack property-proof"));
                assert!(evidence.contains(&format!("receiver rule {id}-")));
                assert!(evidence.contains(&format!("receiver origin external import {origin}")));
            }
        }
        let parity = crate::languages::harness::cold_relations(&fixture.workspace);
        assert_eq!(parity.incremental_verification, Ok(()));
        assert_eq!(parity.cold_verification, Ok(()));
        assert_eq!(parity.incremental, parity.cold);
    }
}

#[test]
fn disabling_configured_factories_removes_plain_read_observations_and_success_targets() {
    let fixture = FactoryWorkspace::new();
    fixture.disable();
    let conn = fixture.workspace.open();
    for (path, ordinary, _, _, _) in configured_sources() {
        let absent: i64 = conn.query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, ordinary], |r| r.get(0),
        ).unwrap();
        assert_eq!(
            absent, 0,
            "disabled mapping has no ordinary property observation: {path}"
        );
        let targets: i64 = conn
            .query_row("SELECT count(*) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=owner_id)=?1 AND kind IN ('calls','references')", [&path], |r| r.get(0))
            .unwrap();
        assert_eq!(targets, 0, "disabled mapping has no success target: {path}");
    }
    let parity = crate::languages::harness::cold_relations(&fixture.workspace);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
}

#[test]
fn configured_properties_remain_noncallable_across_mapping_disable_and_restore() {
    let fixture = FactoryWorkspace::new();
    for phase in 0..3 {
        match phase {
            0 => (),
            1 => fixture.disable(),
            _ => fixture.restore(),
        }
        let conn = fixture.workspace.open();
        for (path, _, direct, _, _) in configured_sources() {
            assert_eq!(
                persisted_coverage(&conn, &path, Some(2), &direct).0,
                "unresolved",
                "{path}: phase {phase}"
            );
        }
    }
}
