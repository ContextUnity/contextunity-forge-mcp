use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_iteration_and_slot_scopes_keep_all_bindings_and_source_coordinates() {
    let source = r#"<template>
  <div v-for="(item, index) in items" :title="item.name">{{ index }}</div>
  <Panel v-slot="{ entry, count }">{{ entry.title }} {{ count }}</Panel>
</template>
<script setup>const items = [];</script>"#;
    let facts = ast::extract("Scoped.vue", "vue", source).unwrap();
    let iteration = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "template_scope"
                && crate::common::assertions::projected_node_details(&facts, node)["bindings"]
                    .as_array()
                    .is_some_and(|names| names.iter().any(|name| name == "item"))
        })
        .unwrap();
    assert!(
        crate::common::assertions::projected_node_details(&facts, iteration)["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "index")
    );
    let index = facts
        .references
        .iter()
        .find(|reference| reference.expression == "index")
        .unwrap();
    assert_eq!(index.source, iteration.id);
    assert_eq!(index.line, 2);
    let line = source.lines().nth(index.line - 1).unwrap();
    assert!(line[index.column..].starts_with("index"));
    let slot = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "template_scope"
                && crate::common::assertions::projected_node_details(&facts, node)["bindings"]
                    .as_array()
                    .is_some_and(|names| names.iter().any(|name| name == "entry"))
        })
        .unwrap();
    assert!(
        crate::common::assertions::projected_node_details(&facts, slot)["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "count")
    );
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.source == slot.id && reference.expression == "entry.title"));
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_iteration_members_resolve_to_declared_element_fields() {
    let w = Workspace::new();
    let source = "<template><ul><li v-for=\"(item, index) in items\">{{ item.title }} {{ index }}</li></ul></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = [];</script>";
    w.write("TypedList.vue", source);
    w.write(
        "TypedCall.vue",
        "<template><button v-for=\"item in items\" @click=\"item.title()\" /></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = [];</script>",
    );
    w.write(
        "ShadowedIterable.vue",
        "<template><div v-for=\"items in groups\"><span v-for=\"item in items\">{{ item.title }}</span></div></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = []; const groups = [];</script>",
    );
    let conn = w.build();
    let (status, evidence): (String, String) = conn.query_row(
        "SELECT r.status, e.evidence FROM resolution_coverage r JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE (SELECT path FROM path_dictionary WHERE path_id=r.path_id)='TypedList.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=r.expression_id)='item.title' AND r.line=1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(status, "resolved", "{evidence}");
    let field_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='TypedList.vue' AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='TypedList.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(field_edge, 1);
    let call_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='TypedCall.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='item.title' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        call_status, "unresolved",
        "a typed field is not a callable method"
    );
    let shadowed_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='ShadowedIterable.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='item.title' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        shadowed_status, "unresolved",
        "outer loop binding shadows the setup iterable"
    );
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_slot_members_follow_imported_component_provider() {
    let w = Workspace::new();
    w.write(
        "app/Panel.vue",
        "<template><slot /></template>\n<script lang=\"ts\">const runtimeTsSlots = defineSlots();</script>\n<script setup lang=\"ts\">interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/JavascriptSlotsPanel.vue",
        "<template><slot /></template>\n<script>const runtimeJsSlots = defineSlots();</script>\n<script setup lang=\"ts\">interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/View.vue",
        "<template><Panel v-slot=\"{ entry }\">{{ entry.title }}</Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write(
        "app/NestedSlot.vue",
        "<template><Panel><template #default=\"{ entry }\">{{ entry.title }}</template></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write(
        "app/JavascriptSlots.vue",
        "<template><JavascriptSlotsPanel v-slot=\"{ entry }\">{{ entry.title }}</JavascriptSlotsPanel></template>\n<script setup lang=\"ts\">import JavascriptSlotsPanel from './JavascriptSlotsPanel.vue';</script>",
    );
    w.write("app/Plain.vue", "<template><slot /></template>");
    w.write(
        "app/AssignedSlotsPanel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">interface Item { title: string } const slots = defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/AssignedSlots.vue",
        "<template><AssignedSlotsPanel v-slot=\"{ entry }\">{{ entry.title }}</AssignedSlotsPanel></template>\n<script setup lang=\"ts\">import AssignedSlotsPanel from './AssignedSlotsPanel.vue';</script>",
    );
    w.write(
        "app/NestedComponent.vue",
        "<template><Panel><Plain><template #default=\"{ entry }\">{{ entry.title }}</template></Plain></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue'; import Plain from './Plain.vue';</script>",
    );
    w.write(
        "app/Unknown.vue",
        "<template><MissingPanel v-slot=\"{ entry }\">{{ entry.title }}</MissingPanel></template>",
    );
    w.write(
        "app/Shadow.vue",
        "<template><Panel v-slot=\"{ entry }\"><span v-for=\"entry in entries\">{{ entry.title }}</span></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue'; const entries = [];</script>",
    );
    w.write(
        "app/FieldCall.vue",
        "<template><Panel v-slot=\"{ entry }\"><button @click=\"entry.title()\" /></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write(
        "app/macro.ts",
        "export function other() {} export function defineSlots() {}",
    );
    w.write(
        "app/ShadowedMacroPanel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">import { other as defineSlots } from './macro'; interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/ShadowedMacro.vue",
        "<template><ShadowedMacroPanel v-slot=\"{ entry }\">{{ entry.title }}</ShadowedMacroPanel></template>\n<script setup lang=\"ts\">import ShadowedMacroPanel from './ShadowedMacroPanel.vue';</script>",
    );
    w.write(
        "app/UnrelatedImportPanel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">import { defineSlots as other } from './macro'; interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/UnrelatedImport.vue",
        "<template><UnrelatedImportPanel v-slot=\"{ entry }\">{{ entry.title }}</UnrelatedImportPanel></template>\n<script setup lang=\"ts\">import UnrelatedImportPanel from './UnrelatedImportPanel.vue';</script>",
    );
    w.write(
        "other/Panel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    let conn = w.build();
    let status = |path: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='entry.title' AND line=1",
            [path], |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing slot member in {path}: {error}"))
    };
    assert_eq!(status("app/View.vue"), "resolved");
    assert_eq!(status("app/NestedSlot.vue"), "resolved");
    assert_eq!(status("app/JavascriptSlots.vue"), "resolved");
    assert_eq!(status("app/UnrelatedImport.vue"), "resolved");
    let field_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/View.vue' AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/Panel.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        field_edge, 1,
        "slot member points to the uniquely imported provider field"
    );
    let nested_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/NestedSlot.vue' AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/Panel.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        nested_edge, 1,
        "nested slot follows the imported parent component"
    );
    for path in [
        "app/Unknown.vue",
        "app/Shadow.vue",
        "app/FieldCall.vue",
        "app/ShadowedMacro.vue",
        "app/NestedComponent.vue",
        "app/AssignedSlots.vue",
    ] {
        assert_eq!(status(path), "unresolved", "{path}");
    }
}
