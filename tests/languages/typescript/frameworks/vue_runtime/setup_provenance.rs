use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_compiler_macros_are_setup_scoped_and_lexically_shadowable() {
    let w = Workspace::new();
    w.write(
        "Macro.vue",
        r#"<script>defineProps();</script><script data-note="a setup b">defineEmits();</script><script setup>defineEmits(); function defineModel() {} defineModel();</script>"#,
    );
    w.write("plain.js", "defineProps();");
    let conn = w.build();

    let count = |expression: &str, status: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Macro.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 AND status=?2",
            rusqlite::params![expression, status],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count("defineProps", "unresolved"), 1);
    let plain_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='plain.js' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineProps'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(plain_status, "unresolved");
    assert_eq!(count("defineEmits", "unresolved"), 1);
    assert_eq!(count("defineEmits", "external"), 1);
    let macro_evidence: String = conn
        .query_row(
            "SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Macro.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineEmits' AND status='external'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        macro_evidence.contains("builtin:vue_macro"),
        "{macro_evidence}"
    );
    assert_eq!(count("defineModel", "resolved"), 1);
    let local_call: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.path_id)='Macro.vue' AND e.kind='calls' AND n.name='defineModel'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(local_call, 1);
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_setup_composable_callable_provenance() {
    let w = Workspace::new();
    w.write(
        "App.vue",
        r#"<template>
  <button @click="emit('save')">Save</button>
</template>
<script setup lang="ts">
const emit = defineEmits(['save']);
emit('save');
</script>"#,
    );
    w.write(
        "Unproven.vue",
        r#"<script setup lang="ts">
function run(emit: (event: string) => void) {
  emit('local');
}
const local = () => {};
let changed = defineEmits(['change']);
changed = local;
changed('change');
emit('unbound');
</script>"#,
    );
    w.write("Other.vue", "<script setup>emit('other');</script>");
    w.write(
        "MacroAlias.vue",
        "<script setup>const customEmit = defineEmits(['save']); customEmit('save');</script>",
    );
    w.write("Reassigned.vue", "<script setup>const emit = defineEmits(['save']); const local = () => {}; emit = local; emit('save');</script>");
    w.write("Duplicate.vue", "<script setup>const emit = defineEmits(['one']); const emit = defineEmits(['two']); emit('two');</script>");
    w.write(
        "Script.vue",
        "<script>const emit = defineEmits(['save']); emit('ordinary');</script>",
    );
    w.write("plain.ts", "emit('plain');");
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> Vec<(i64, String, String)> {
        let mut statement = conn.prepare(
            "SELECT line,status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 ORDER BY line",
        ).unwrap();
        statement
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };

    let rows = coverage("App.vue", "defineEmits");
    assert_eq!(rows.len(), 1, "producer defineEmits: {rows:?}");
    assert_eq!(rows[0].1, "external", "producer defineEmits: {rows:?}");
    assert!(
        rows[0].2.contains("builtin:vue_macro"),
        "producer defineEmits: {rows:?}"
    );

    let emit_rows = coverage("App.vue", "emit");
    assert_eq!(
        emit_rows.len(),
        2,
        "script and template calls of emit: {emit_rows:?}"
    );
    assert!(
        emit_rows.iter().any(|row| row.0 == 2),
        "template call of emit: {emit_rows:?}"
    );
    assert!(
        emit_rows.iter().any(|row| row.0 >= 6),
        "script call of emit: {emit_rows:?}"
    );
    for row in emit_rows {
        assert_eq!(row.1, "external", "emit: {row:?}");
        assert!(row.2.contains("builtin:vue_macro"), "emit: {row:?}");
    }

    let macro_alias = coverage("MacroAlias.vue", "customEmit");
    assert_eq!(macro_alias.len(), 1);
    assert_eq!(macro_alias[0].1, "external");
    assert!(
        macro_alias[0].2.contains("builtin:vue_macro"),
        "{macro_alias:?}"
    );

    for (path, expression) in [
        ("Unproven.vue", "emit"),
        ("Unproven.vue", "changed"),
        ("Other.vue", "emit"),
        ("Script.vue", "emit"),
        ("plain.ts", "emit"),
        ("Reassigned.vue", "emit"),
        ("Duplicate.vue", "emit"),
    ] {
        let rows = coverage(path, expression);
        assert!(!rows.is_empty(), "missing callable {path}:{expression}");
        assert!(
            rows.iter().all(|row| !row.2.contains("builtin:vue_macro")),
            "unproven callable acquired builtin:vue_macro: {path}:{expression} {rows:?}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_setup_inherits_javascript_web_builtins_as_external() {
    let source = r#"<script setup lang="ts">
function run() {
    fetch('/api');
    const items: Array<string> = [];
    items.map(item => item);
}
</script>"#;
    let facts = ast::extract("Platform.vue", "vue", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "Platform.vue".to_owned(),
            facts,
        )]));
    for expression in ["items.map", "fetch"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing Vue coverage for {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        let expected_origin = if expression == "fetch" {
            "builtin:web_api"
        } else {
            "JavaScript/Web platform"
        };
        assert!(
            coverage.evidence.contains(expected_origin),
            "{expression}: {coverage:#?}"
        );
    }
}
