use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_transitive_nested_defineprops_interfaces() {
    let w = Workspace::new();
    w.write(
        "app/types.ts",
        r#"export interface Citation {
  url: string;
  summary: string;
}
export interface BookmarkItem {
  id: string;
  type: string;
  title: string;
  citation: Citation;
}
export interface Props {
  bookmark: BookmarkItem;
}
"#,
    );
    w.write(
        "app/InlineProps.vue",
        r#"<template>
  <p>{{ props.bookmark.title }}</p>
  <p>{{ props.bookmark.citation.url }}</p>
  <p>{{ props.bookmark.citation?.summary }}</p>
  <p>{{ props.bookmark.missing }}</p>
</template>
<script setup lang="ts">
import type { BookmarkItem } from './types';
const props = defineProps<{
  bookmark: BookmarkItem;
}>();
</script>
"#,
    );
    w.write(
        "app/NamedProps.vue",
        r#"<template>
  <p>{{ props.bookmark.citation.url }}</p>
</template>
<script setup lang="ts">
import type { Props } from './types';
const props = defineProps<Props>();
</script>
"#,
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };

    let title_status = coverage("app/InlineProps.vue", "props.bookmark.title").0;
    assert_eq!(title_status, "resolved", "props.bookmark.title");

    let url_cov = conn.query_row(
        "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.citation.url'",
        [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).unwrap();
    assert_eq!(url_cov.0, "resolved", "props.bookmark.citation.url");

    let summary_cov = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.citation?.summary'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(summary_cov, "resolved", "props.bookmark.citation?.summary");

    let missing_status = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.missing'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(
        missing_status, "unresolved",
        "props.bookmark.missing must remain unresolved"
    );

    let named_url_status = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/NamedProps.vue' AND x.expression='props.bookmark.citation.url'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(
        named_url_status, "resolved",
        "props.bookmark.citation.url in NamedProps.vue"
    );

    let edge_count: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/InlineProps.vue' AND e.kind='references' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/types.ts' AND dst.kind='field' AND dst.name='url'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        edge_count, 1,
        "reference edge to Citation.url field in app/types.ts"
    );
}
