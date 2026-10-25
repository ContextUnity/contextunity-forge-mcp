use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_components_directory_resolves_with_project_scope() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/nuxt/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/nuxt/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/nuxt/components/Base/WidgetCard.vue", "<template><article /></template>"),
        ("apps/nuxt/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/other/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/other/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/other/components/Base/WidgetCard.vue", "<template><aside /></template>"),
        ("apps/other/pages/index.vue", "<template><base-widget-card /></template>"),
        ("apps/plain/package.json", r#"{"dependencies":{"vue":"^3"}}"#),
        ("apps/plain/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/plain/components/Base/WidgetCard.vue", "<template><footer /></template>"),
        ("apps/plain/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/disabled/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/disabled/nuxt.config.ts", "export default defineNuxtConfig({ components: false });"),
        ("apps/disabled/components/Base/WidgetCard.vue", "<template><header /></template>"),
        ("apps/disabled/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/collision/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/collision/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/collision/components/Base/WidgetCard.vue", "<template><article /></template>"),
        ("apps/collision/components/BaseWidgetCard.vue", "<template><aside /></template>"),
        ("apps/collision/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/dynamic/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/dynamic/nuxt.config.ts", "const options = { components: false }; export default defineNuxtConfig({ ...options });"),
        ("apps/dynamic/components/Base/WidgetCard.vue", "<template><nav /></template>"),
        ("apps/dynamic/pages/index.vue", "<template><BaseWidgetCard /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let status = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("missing Nuxt template tag {path}:{expression}: {error}"))
    };
    for (page, tag, component) in [
        (
            "apps/nuxt/pages/index.vue",
            "BaseWidgetCard",
            "apps/nuxt/components/Base/WidgetCard.vue",
        ),
        (
            "apps/other/pages/index.vue",
            "base-widget-card",
            "apps/other/components/Base/WidgetCard.vue",
        ),
    ] {
        assert_eq!(status(page, tag), "resolved", "{page}");
        let exact_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2",
            rusqlite::params![page, component],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(exact_edge, 1, "{page} must point to its own Nuxt component");
    }
    for page in [
        "apps/plain/pages/index.vue",
        "apps/disabled/pages/index.vue",
        "apps/dynamic/pages/index.vue",
    ] {
        assert_eq!(status(page, "BaseWidgetCard"), "unresolved", "{page}");
    }
    assert_eq!(
        status("apps/collision/pages/index.vue", "BaseWidgetCard"),
        "ambiguous"
    );
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_builtin_components_require_project_provenance() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/nuxt/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/nuxt/nuxt.config.ts", "export default defineNuxtConfig({ components: false });"),
        ("apps/nuxt/app.vue", "<template><NuxtLayout><NuxtPage /></NuxtLayout></template>"),
        ("apps/nuxt/pages/index.vue", "<template><ClientOnly><span /></ClientOnly><UnknownWidget /></template>"),
        ("apps/nuxt/pages/local.vue", "<template><ClientOnly /></template><script setup>import ClientOnly from '../widgets/ClientOnly.vue';</script>"),
        ("apps/nuxt/widgets/ClientOnly.vue", "<template><div /></template>"),
        ("apps/plain/package.json", r#"{"dependencies":{"vue":"^3"}}"#),
        ("apps/plain/pages/index.vue", "<template><ClientOnly /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=1",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    let (status, evidence) = coverage("apps/nuxt/pages/index.vue", "ClientOnly");
    assert_eq!(status, "external");
    assert!(evidence.contains("builtin:nuxt"), "{evidence}");
    for tag in ["NuxtLayout", "NuxtPage"] {
        let (status, evidence) = coverage("apps/nuxt/app.vue", tag);
        assert_eq!(status, "external", "{tag}");
        assert!(evidence.contains("builtin:nuxt"), "{tag}: {evidence}");
    }
    assert_eq!(
        coverage("apps/nuxt/pages/index.vue", "UnknownWidget").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/plain/pages/index.vue", "ClientOnly").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/nuxt/pages/local.vue", "ClientOnly").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/nuxt/pages/local.vue' AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='apps/nuxt/widgets/ClientOnly.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "explicit local component takes priority over Nuxt built-in"
    );
}

#[cfg(feature = "lang-vue")]
#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_module_components_require_registered_provider() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/radix/package.json", r#"{"dependencies":{"nuxt":"^3","radix-vue":"^1"}}"#),
        ("apps/radix/nuxt.config.ts", "export default defineNuxtConfig({ modules: ['radix-vue/nuxt'] });"),
        ("apps/radix/pages/index.vue", "<template><DialogRoot><DialogPortal><DialogOverlay /><DialogContent><DialogTitle /><DialogDescription /><DialogClose /></DialogContent></DialogPortal></DialogRoot><UnknownRadixTag /></template>"),
        ("apps/radix/pages/local.vue", "<template><DialogTitle /></template><script setup>import DialogTitle from '../widgets/DialogTitle.vue';</script>"),
        ("apps/radix/widgets/DialogTitle.vue", "<template><h1 /></template>"),
        ("apps/disabled/package.json", r#"{"dependencies":{"nuxt":"^3","radix-vue":"^1"}}"#),
        ("apps/disabled/nuxt.config.ts", "export default defineNuxtConfig({ modules: [] });"),
        ("apps/disabled/pages/index.vue", "<template><DialogTitle /></template>"),
        ("apps/undeclared/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/undeclared/nuxt.config.ts", "export default defineNuxtConfig({ modules: ['radix-vue/nuxt'] });"),
        ("apps/undeclared/pages/index.vue", "<template><DialogTitle /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=1",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for tag in [
        "DialogRoot",
        "DialogPortal",
        "DialogOverlay",
        "DialogContent",
        "DialogTitle",
        "DialogDescription",
        "DialogClose",
    ] {
        let (status, evidence) = coverage("apps/radix/pages/index.vue", tag);
        assert_eq!(status, "external", "{tag}");
        assert!(evidence.contains("radix-vue/nuxt"), "{tag}: {evidence}");
    }
    assert_eq!(
        coverage("apps/radix/pages/index.vue", "UnknownRadixTag").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/disabled/pages/index.vue", "DialogTitle").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/undeclared/pages/index.vue", "DialogTitle").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/radix/pages/local.vue", "DialogTitle").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/radix/pages/local.vue' AND e.kind='references' AND e.line=1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='apps/radix/widgets/DialogTitle.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "explicit local component takes priority over registered Nuxt module"
    );
}
