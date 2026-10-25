use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_options_api_registered_components_use_imported_aliases() {
    let w = Workspace::new();
    w.write("LeafCard.vue", "<template><article /></template>");
    w.write(
        "Plain.vue",
        "<template><card-alias /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: { CardAlias: LeafCard } };</script>",
    );
    w.write(
        "Wrapped.vue",
        "<template><WrappedAlias /></template>\n<script lang=\"ts\">import { defineComponent } from 'vue'; import LeafCard from './LeafCard.vue'; export default defineComponent({ components: { WrappedAlias: LeafCard } });</script>",
    );
    w.write(
        "Unregistered.vue",
        "<template><orphan-card /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: {} };</script>",
    );
    w.write(
        "UnregisteredImport.vue",
        "<template><LeafCard /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { data() { return {}; } };</script>",
    );
    w.write(
        "NoVueWrapper.vue",
        "<template><SpoofAlias /></template>\n<script>import LeafCard from './LeafCard.vue'; function defineComponent(options) { return options; } export default defineComponent({ components: { SpoofAlias: LeafCard } });</script>",
    );
    w.write(
        "Duplicate.vue",
        "<template><DuplicateAlias /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: { DuplicateAlias: LeafCard, DuplicateAlias: LeafCard } };</script>",
    );
    let conn = w.build();
    let status = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("missing template component {path}:{expression}: {error}"))
    };
    for path in ["Plain.vue", "Wrapped.vue"] {
        assert_eq!(status(path, "LeafCard"), "resolved", "{path}");
        let template_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='LeafCard.vue'",
            [path],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            template_edge, 1,
            "{path} template tag points to the imported component file"
        );
    }
    assert_eq!(status("Unregistered.vue", "orphan-card"), "unresolved");
    assert_eq!(status("UnregisteredImport.vue", "LeafCard"), "unresolved");
    assert_eq!(status("NoVueWrapper.vue", "SpoofAlias"), "unresolved");
    assert_eq!(status("Duplicate.vue", "DuplicateAlias"), "unresolved");
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_sfc_template_bridge_resolves_setup_bindings_and_component_imports() {
    let w = Workspace::new();
    w.write(
        "MyButton.vue",
        "<template><button /></template><script setup>const label = 'ok';</script>",
    );
    w.write(
        "App.vue",
        r#"<template>
  <my-button @click="worker.run" />
  <button @click="worker.run()" />
  <p>{{ buttonText }}</p>
  <p>{{ hidden }}</p>
</template>
<script setup lang="ts">
import MyButton from './MyButton.vue';
class Worker { run() { return 1; } }
const worker = new Worker();
const buttonText = ref('Ready');
function onlyInside() { const hidden = 'private'; return hidden; }
</script>
"#,
    );
    let facts = ast::extract(
        "App.vue",
        "vue",
        &fs::read_to_string(w.0.join("App.vue")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        facts
            .references
            .iter()
            .filter(|reference| reference.kind == "calls" && reference.expression == "worker.run")
            .count(),
        2,
        "{facts:#?}"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| { reference.expression == "buttonText" && reference.line == 4 }),
        "template interpolation retains its setup binding reference: {facts:#?}"
    );
    let conn = w.build();
    let calls: i64 = conn.query_row(
        "SELECT COALESCE(sum(e.occurrence_count),0) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.path_id)='App.vue' AND e.kind='calls' AND n.name='run'",
        [], |row| row.get(0),
    ).unwrap();
    let mut statement = conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),line,(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.evidence_id) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)='App.vue' AND kind='calls'").unwrap();
    let occurrences: Vec<(String, String, usize, String)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(calls, 2, "{occurrences:#?}");
    let components: i64 = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='MyButton' AND status='resolved'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(components, 1);
    let component_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE e.kind='imports' AND src.id='module:App.vue' AND dst.id='module:MyButton.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        component_edge, 1,
        "local component import links to MyButton.vue"
    );
    let mut binding_statement = conn.prepare(
        "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='buttonText' AND line=4",
    ).unwrap();
    let binding_coverages: Vec<(String, String)> = binding_statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        binding_coverages
            .iter()
            .any(|(status, _)| status == "resolved"),
        "template interpolation resolves to the local setup binding: {binding_coverages:#?}"
    );
    let hidden_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='hidden' AND line=5",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        hidden_status, "unresolved",
        "nested script local is outside template scope"
    );
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_sfc_template_bridge_resolves_large_setup_scope() {
    let declarations = (0..192)
        .map(|index| format!("const item{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        "<template><p>{{{{ item191 }}}}</p></template><script setup>{declarations}</script>"
    );
    let facts = ast::extract("Large.vue", "vue", &source).unwrap();
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "Large.vue".to_owned(),
            facts,
        )]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| { coverage.expression == "item191" && coverage.status == "resolved" }),
        "last setup declaration resolves in template: {graph:#?}"
    );
}
