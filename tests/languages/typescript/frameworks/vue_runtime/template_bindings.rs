use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_bindings() {
    let w = Workspace::new();
    w.write(
        "MyButton.vue",
        r#"<template>
  <button>{{ label }}</button>
</template>
<script setup lang="ts">
defineProps<{ label: string }>();
</script>
"#,
    );
    w.write(
        "App.vue",
        r#"<template>
  <div>
    <MyButton :label="buttonText" @click="onSubmit" />
    <p>{{ user.name }}</p>
  </div>
</template>
<script setup lang="ts">
import MyButton from './MyButton.vue';
const buttonText = "Click me";
function onSubmit() {
    return 42;
}

const user = { name: "Antigravity" };
ref(); computed(); reactive(); shallowRef(); shallowReactive(); toRef(); toRefs(); unref(); isRef();
watch(); watchEffect(); onMounted(); onUnmounted(); onUpdated(); onBeforeMount(); onBeforeUnmount();
nextTick(); inject(); provide(); useSlots(); useAttrs(); useI18n(); t();
</script>
"#,
    );

    let facts = ast::extract(
        "App.vue",
        "vue",
        &fs::read_to_string(w.0.join("App.vue")).unwrap(),
    )
    .unwrap();

    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        expressions.contains(&"MyButton"),
        "missing template custom component MyButton: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"buttonText"),
        "missing template bound prop buttonText: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"onSubmit"),
        "missing template event handler onSubmit: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"user"),
        "missing template interpolation user: {:?}",
        expressions
    );
    let conn = w.build();
    for name in "ref computed reactive shallowRef shallowReactive toRef toRefs unref isRef watch watchEffect onMounted onUnmounted onUpdated onBeforeMount onBeforeUnmount nextTick inject provide useSlots useAttrs useI18n".split_ascii_whitespace() {
        let (status, evidence): (String, String) = conn
            .query_row(
                "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or_else(|error| panic!("missing Vue runtime call {name}: {error}"));
        assert_eq!(status, "external", "Vue runtime call {name}");
        assert!(evidence.contains("builtin:vue_runtime"), "Vue runtime call {name}: {evidence}");
    }
    let unbound_t: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='t'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        unbound_t, "unresolved",
        "t requires a proven callable setup binding"
    );
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_defineprops_template_members() {
    let w = Workspace::new();
    w.write(
        "Props.vue",
        r#"<template>
  <p>{{ props.label }}</p>
  <p>{{ props.missing }}</p>
</template>
<script setup lang="ts">
const props = defineProps<{ label: string }>();
</script>"#,
    );
    w.write(
        "Shadow.vue",
        r#"<template><div v-for="props in items">{{ props.label }}</div></template>
<script setup lang="ts">const props = defineProps<{ label: string }>(); const items = [{}];</script>"#,
    );
    w.write(
        "Sibling.vue",
        "<template><p>{{ props.label }}</p></template><script setup>const other = 1;</script>",
    );
    w.write(
        "Duplicate.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string; label: string; label: string }>();</script>",
    );
    w.write(
        "Reassigned.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string }>(); props = { label: 'local' };</script>",
    );
    w.write(
        "DuplicateBinding.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string }>(); const props = defineProps<{ label: string }>();</script>",
    );
    w.write(
        "ShadowedProducer.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">function defineProps() { return { label: 'local' }; } const props = defineProps<{ label: string }>();</script>",
    );
    w.write(
        "Ordinary.vue",
        "<template><p>{{ props.label }}</p></template><script lang=\"ts\">const props = defineProps<{ label: string }>();</script>",
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(coverage("Props.vue", "props.label"), "resolved");
    assert_eq!(coverage("Props.vue", "props.missing"), "unresolved");
    assert_eq!(coverage("Shadow.vue", "props.label"), "unresolved");
    assert_eq!(coverage("Sibling.vue", "props.label"), "unresolved");
    for path in [
        "Duplicate.vue",
        "Reassigned.vue",
        "DuplicateBinding.vue",
        "ShadowedProducer.vue",
        "Ordinary.vue",
    ] {
        assert_eq!(
            coverage(path, "props.label"),
            "unresolved",
            "unproven typed props in {path}"
        );
    }
    let exact_field_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='Props.vue' AND e.kind='references' AND e.line=2 AND (SELECT path FROM path_dictionary WHERE path_id=src.path_id)='Props.vue' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='Props.vue' AND dst.kind='field' AND dst.name='label'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_field_edges, 1,
        "template member must link to the same-file indexed label field"
    );
    for path in [
        "Duplicate.vue",
        "Reassigned.vue",
        "DuplicateBinding.vue",
        "ShadowedProducer.vue",
        "Ordinary.vue",
    ] {
        let field_edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?1 AND dst.name='label'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            field_edges, 0,
            "unproven typed props cannot provide an exact template target in {path}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_nested_and_optional_defineprops_template_members() {
    let w = Workspace::new();
    w.write(
        "Nested.vue",
        r#"<template>
  <p>{{ props.user.profile.name }}</p>
  <p>{{ props.user?.profile?.name }}</p>
  <p>{{ props.user.profile.missing }}</p>
</template>
<script setup lang="ts">
const props = defineProps<{ user?: { profile: { name?: string } } }>();
</script>"#,
    );
    w.write(
        "Shadow.vue",
        r#"<template><div v-for="props in items">{{ props.user.profile.name }}</div></template>
<script setup lang="ts">const props = defineProps<{ user: { profile: { name: string } } }>(); const items = [];</script>"#,
    );
    w.write(
        "Sibling.vue",
        r#"<template>{{ props.user.profile.name }}</template><script setup lang="ts">const other = 1;</script>"#,
    );
    w.write(
        "SameLine.vue",
        r#"<template>{{ props.left.name }} {{ props.right.name }}</template><script setup lang="ts">const props = defineProps<{ left: { name: string }; right: { name: string } }>();</script>"#,
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for expression in ["props.user.profile.name", "props.user?.profile?.name"] {
        assert_eq!(
            coverage("Nested.vue", expression),
            "resolved",
            "{expression}"
        );
    }
    assert_eq!(
        coverage("Nested.vue", "props.user.profile.missing"),
        "unresolved"
    );
    assert_eq!(
        coverage("Shadow.vue", "props.user.profile.name"),
        "unresolved"
    );
    assert_eq!(
        coverage("Sibling.vue", "props.user.profile.name"),
        "unresolved"
    );
    for expression in ["props.left.name", "props.right.name"] {
        assert_eq!(
            coverage("SameLine.vue", expression),
            "unresolved",
            "ambiguous type-literal field {expression}"
        );
    }
    let exact_field_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='Nested.vue' AND e.kind='references' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='Nested.vue' AND dst.kind='field' AND dst.name='name'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_field_edges, 2,
        "both template expressions link to their same-file typed field"
    );
}
