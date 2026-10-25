use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_composable_return_members_follow_setup_aliases() {
    let w = Workspace::new();
    w.write(
        "Good.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    w.write(
        "Arrow.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { const save = () => 1; return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    w.write(
        "Unknown.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>const alias = unknownFactory();</script>",
    );
    w.write(
        "Shadow.vue",
        "<template><div v-for=\"alias in aliases\"><button @click=\"alias.save\" /></div></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); const alias = actions; const aliases = [];</script>",
    );
    w.write(
        "Rebound.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); let alias = actions; alias = {};</script>",
    );
    w.write(
        "MutatedMember.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } save = () => 2; return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    let conn = w.build();
    let status = |path: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='alias.save' AND line=1",
            [path], |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing alias.save in {path}: {error}"))
    };
    for path in ["Good.vue", "Arrow.vue"] {
        assert_eq!(status(path), "resolved", "{path}");
        let exact_save_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?1 AND dst.kind='function' AND dst.name='save'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            exact_save_edge, 1,
            "{path} template alias.save points to its returned local function"
        );
    }
    for path in [
        "Unknown.vue",
        "Shadow.vue",
        "Rebound.vue",
        "MutatedMember.vue",
    ] {
        assert_eq!(status(path), "unresolved", "{path}");
    }
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_imported_composable_return_members_follow_unique_provider() {
    let w = Workspace::new();
    w.write(
        "app/actions.ts",
        "export function buildActions() { function save() { return 1; } return { save }; }",
    );
    w.write(
        "app/View.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup lang=\"ts\">import { buildActions } from './actions'; const actions = buildActions(); const alias = actions;</script>",
    );
    w.write(
        "other/actions.ts",
        "export function buildActions() { function save() { return 2; } return { save }; }",
    );
    let conn = w.build();
    let status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/View.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='alias.save' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(status, "resolved");
    let exact_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/View.vue' AND e.kind='calls' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/actions.ts' AND dst.kind='function' AND dst.name='save'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_edge, 1,
        "template call points to the uniquely imported provider"
    );
}
