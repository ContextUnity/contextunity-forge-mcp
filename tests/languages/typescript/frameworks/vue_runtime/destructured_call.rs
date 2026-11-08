use super::super::*;

#[cfg(feature = "lang-vue")]
#[test]
fn vue_destructured_composable_call_uses_indexed_return_member() {
    use contextunity_forge_mcp::core::commitments;

    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write(
        "app/i18n.ts",
        "export function useI18n() { function t(key: string) { return key; } return { t }; }",
    );
    w.write(
        "app/View.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; const { t } = useI18n();</script>",
    );
    w.write(
        "app/Alias.vue",
        "<template><p>{{ translate('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; const { t: translate } = useI18n();</script>",
    );
    w.write(
        "app/Unknown.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">const { t } = useI18n();</script>",
    );
    w.write(
        "app/Rebound.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; let { t } = useI18n(); t = () => 'changed';</script>",
    );
    w.write(
        "app/TypeOnly.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">\nimport type { useI18n } from './i18n';\nconst { t } = useI18n();\n</script>",
    );
    w.write(
        "app/SpecifierTypeOnly.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">\nimport { type useI18n } from './i18n';\nconst { t } = useI18n();\n</script>",
    );
    w.write(
        "other/i18n.ts",
        "export function useI18n() { function t(key: string) { return key; } return { t }; }",
    );
    w.build();
    let conn = w.open();
    commitments::verify(&conn).unwrap();
    for (path, expression) in [("app/View.vue", "t"), ("app/Alias.vue", "translate")] {
        let status: String = query_scalar(
            &conn,
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression],
        );
        assert_eq!(status, "resolved", "{path} {expression}");
        let edge_count: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND e.kind='calls' AND dst.name='t'",
            rusqlite::params![path, "app/i18n.ts"],
        );
        assert_eq!(edge_count, 1, "{path} call edge count to t");
    }
    for path in [
        "app/Unknown.vue",
        "app/Rebound.vue",
        "app/TypeOnly.vue",
        "app/SpecifierTypeOnly.vue",
    ] {
        let status: String = query_scalar(
            &conn,
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='t' AND line=1",
            rusqlite::params![path],
        );
        assert!(
            matches!(status.as_str(), "unresolved" | "ambiguous"),
            "{path} t status={status}"
        );
        let edge_count: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/i18n.ts' AND e.kind='calls' AND dst.name='t'",
            rusqlite::params![path],
        );
        assert_eq!(edge_count, 0, "{path} negative call edge count to t");
    }

    drop(conn);
    // Observed Library UI-store body, retained verbatim; existing controls above
    // continue to prove local imported composables and their negative cases.
    let provider = r####"/**
 * UI Store
 *
 * Centralized UI state management for modals, sidebar, and other UI elements.
 * Prevents prop drilling for modal visibility and other transient UI states.
 *
 * @example
 * ```ts
 * const uiStore = useUiStore();
 * uiStore.openModal('settings');
 * uiStore.toggleSidebar();
 * ```
 */

export type ModalType = 'settings' | 'admin' | 'integrations' | null;

export const useUiStore = defineStore("ui", () => {
  const auth = useAuth();

  // Sidebar state
  const _sidebarOpen = ref(false);

  // Modal state (only one modal open at a time)
  const _activeModal = ref<ModalType>(null);

  // Computed getters
  const sidebarOpen = computed(() => _sidebarOpen.value);
  const activeModal = computed(() => _activeModal.value);
  const isSettingsOpen = computed(() => _activeModal.value === 'settings');
  const isAdminOpen = computed(() => _activeModal.value === 'admin');
  const isIntegrationsOpen = computed(() => _activeModal.value === 'integrations');

  // Sidebar actions
  function openSidebar(): void {
    _sidebarOpen.value = true;
  }

  function closeSidebar(): void {
    _sidebarOpen.value = false;
  }

  function toggleSidebar(): void {
    _sidebarOpen.value = !_sidebarOpen.value;
  }

  function setSidebarOpen(value: boolean): void {
    _sidebarOpen.value = value;
  }

  // Modal actions
  function openModal(modal: ModalType): void {
    // Prevent non-admins from opening admin modal
    if (modal === 'admin' && !auth.isAdmin.value) {
      return;
    }
    _activeModal.value = modal;
  }

  function closeModal(): void {
    _activeModal.value = null;
  }

  function openSettings(): void {
    _activeModal.value = 'settings';
  }

  function openAdmin(): void {
    // Prevent non-admins from opening admin modal
    if (!auth.isAdmin.value) {
      return;
    }
    _activeModal.value = 'admin';
  }

  function openIntegrations(): void {
    _activeModal.value = 'integrations';
  }

  return {
    // Computed getters
    sidebarOpen,
    activeModal,
    isSettingsOpen,
    isAdminOpen,
    isIntegrationsOpen,

    // Sidebar actions
    openSidebar,
    closeSidebar,
    toggleSidebar,
    setSidebarOpen,

    // Modal actions
    openModal,
    closeModal,
    openSettings,
    openAdmin,
    openIntegrations,
  };
});
"####;
    w.write(
        "package.json",
        r#"{"dependencies":{"nuxt":"3","pinia":"3","@pinia/nuxt":"1","@nuxtjs/i18n":"9"}}"#,
    );
    let active_config = "export default defineNuxtConfig({components:false, modules:['@pinia/nuxt','@nuxtjs/i18n']});\n";
    w.write("nuxt.config.ts", active_config);
    w.write(
        ".forge/frameworks/nuxt.toml",
        r##"replace=true
[[syntax_rules]]
id="configured-environment"
language="typescript"
operation={kind="component_environment",package="nuxt",config_file="nuxt.config.ts",config_callee="defineNuxtConfig",enabled_key="components",modules_key="modules",directory="components",extension=".vue",virtual_module="#components",autoimports=[],builtins=[],modules={"@pinia/nuxt"={package="pinia",components=[],exports=[{directory="stores",extension=".ts"}]},"@nuxtjs/i18n"={package="@nuxtjs/i18n",components=[],call_results={useI18n=["t"]}}}}
[[syntax_rules]]
id="configured-factory"
language="typescript"
operation={kind="factory_members",callee={kind="global",name="defineStore"},argument=1,shape="setup_return",configured_module="@pinia/nuxt"}
"##,
    );
    let configured = "<script setup lang=\"ts\">\nconst uiStore = useUiStore();\nconst { t } = useI18n();\nconst { toast } = useToast();\nuiStore.toggleSidebar();\nasync function handleCopy() {\n  const success = await copyToClipboard(props.text);\n  if (success) {\n    toast.success(t('chat.actions.copied'), t('chat.actions.copiedDescription'));\n  } else {\n    toast.error(t('chat.actions.copyFailed'), t('chat.actions.copyFailedDescription'));\n  }\n}\n</script>\n<template>{{ t('save') }}</template>\n";
    w.write("app/Configured.vue", configured);
    let aliases = "export const useAliasStore = defineStore('alias', () => { const state = ref(0); function close() {} function abort() {} return { dismiss: close, state, abort: stream.cancel }; });\n";
    w.write("stores/aliases.ts", aliases);
    let result_guards = "<script setup lang='ts'>\nconst store = useAliasStore();\nconst {t: translate} = useI18n();\nconst say = translate;\nsay('save');\nstore.dismiss();\nstore.state();\nstore.abort();\nfunction shadow(t: any) { t('shadow'); }\nfunction parameter(useI18n: any) { const {t} = useI18n(); t('parameter'); }\nfunction rebound() { let {t} = useI18n(); t = other; t('rebound'); }\n</script>\n";
    w.write("app/ResultGuards.vue", result_guards);
    let observe = |conn: &rusqlite::Connection, provider_present: bool, module_present: bool| {
        let errors: i64 = query_scalar(conn, "SELECT count(*) FROM errors", []);
        assert_eq!(errors, 0, "configured result descriptors are valid");
        for (expression, expected) in [
            (
                "uiStore.toggleSidebar",
                if provider_present && module_present {
                    "resolved"
                } else {
                    "unresolved"
                },
            ),
            (
                "t",
                if module_present {
                    "external"
                } else {
                    "unresolved"
                },
            ),
        ] {
            let statuses: Vec<String> = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/Configured.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 ORDER BY line").unwrap().query_map([expression], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert!(!statuses.is_empty(), "{expression}: retained raw identity");
            assert!(
                statuses.iter().all(|status| status == expected),
                "{expression}: {statuses:?}, expected {expected}"
            );
        }
        let actual: Vec<String> = conn.prepare("SELECT dst.id FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/Configured.vue' AND e.kind='calls' AND dst.name='toggleSidebar' ORDER BY dst.id").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        if provider_present && module_present {
            let target: String = query_scalar(
                conn,
                "SELECT id FROM nodes WHERE (SELECT path FROM path_dictionary WHERE path_id=nodes.path_id)='stores/ui.ts' AND name='toggleSidebar'",
                [],
            );
            assert_eq!(actual, vec![target], "actual indexed callable target");
        } else {
            assert!(actual.is_empty(), "no guessed setup target");
        }
        let fake_t: i64 = query_scalar(
            conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/Configured.vue' AND e.kind='calls' AND dst.name='t'",
            [],
        );
        assert_eq!(fake_t, 0, "external result never becomes a local t edge");
        for (expression, expected) in [
            (
                "say",
                if module_present {
                    "external"
                } else {
                    "unresolved"
                },
            ),
            (
                "store.dismiss",
                if module_present {
                    "resolved"
                } else {
                    "unresolved"
                },
            ),
            ("store.state", "unresolved"),
            ("store.abort", "unresolved"),
            ("t", "unresolved"),
        ] {
            let statuses: Vec<String> = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/ResultGuards.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 ORDER BY line").unwrap().query_map([expression], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert!(!statuses.is_empty(), "guard identity {expression}");
            assert!(
                statuses.iter().all(|status| status == expected),
                "{expression}: {statuses:?} expected {expected}"
            );
        }
        let alias_edges: Vec<String> = conn.prepare("SELECT dst.id FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/ResultGuards.vue' AND e.kind='calls' AND dst.name='close'").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        if module_present {
            let canonical: String = query_scalar(
                conn,
                "SELECT id FROM nodes WHERE (SELECT path FROM path_dictionary WHERE path_id=nodes.path_id)='stores/aliases.ts' AND name='close'",
                [],
            );
            assert_eq!(
                alias_edges,
                [canonical],
                "public alias retains actual declaration ID"
            );
        } else {
            assert!(alias_edges.is_empty());
        }
    };
    w.build();
    let initial_connection = w.open();
    commitments::verify(&initial_connection).unwrap();
    observe(&initial_connection, false, true);
    let cold_references = crate::languages::support::ColdFilesystemReferenceCache::default();
    w.write("stores/ui.ts", provider);
    w.delta(&["stores/ui.ts"]);
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    observe(&parity.incremental_connection, true, true);
    w.write(
        "nuxt.config.ts",
        "export default defineNuxtConfig({components:false, modules:[]});\n",
    );
    w.delta(&["nuxt.config.ts"]);
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    observe(&parity.incremental_connection, true, false);
    w.write("nuxt.config.ts", active_config);
    w.delta(&["nuxt.config.ts"]);
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    observe(&parity.incremental_connection, true, true);
    // Observed Library health.ts:327-332 and messages.ts:67-76 initialization.
    // Reuse the configured provider and exact target/uncertainty observations.
    let consumer_status = |conn: &rusqlite::Connection, path: &str, expected: &str| {
        let statuses: Vec<String> = conn
            .prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='uiStore.toggleSidebar'")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!statuses.is_empty(), "{path}: retained raw identity");
        assert!(
            statuses.iter().all(|status| status == expected),
            "{path}: {statuses:?}, expected {expected}"
        );
        let calls: i64 = query_scalar(
            conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND dst.name='toggleSidebar'",
            [path],
        );
        if expected == "resolved" {
            assert_eq!(calls, 1, "{path} call edge count to toggleSidebar");
        } else {
            assert_eq!(calls, 0, "{path} negative call edge count to toggleSidebar");
        }
    };
    let initializations = [
        r#"  if (import.meta.client) {
    // Initialize from browser status (so first paint doesn't assume online).
    _isOnline.value = navigator.onLine;
    window.addEventListener("online", () => setOnline(true));
    window.addEventListener("offline", () => setOnline(false));
  }
"#,
        r#"  if (typeof window !== "undefined") {
    try {
      const savedThread = sessionStorage.getItem(STORAGE_KEYS.THREAD_ID);
      const savedMessages = sessionStorage.getItem(STORAGE_KEYS.MESSAGES);
      if (savedThread) threadId.value = savedThread;
      if (savedMessages) messages.value = parseMessages(JSON.parse(savedMessages) as unknown);
    } catch {
      messages.value = [];
    }
  }
"#,
    ];
    let unsupported_bodies = [
        provider.replace("  return {", "  return { ...unknown,"),
        provider.replace("    toggleSidebar,", "    toggleSidebar, toggleSidebar,"),
        provider.replace("    toggleSidebar,", "    [key]: toggleSidebar,"),
        provider.replace(
            "  const auth =",
            "  if (condition) return {};\n  const auth =",
        ),
        provider.replace("  return {", "  toggleSidebar = other;\n  return {"),
        provider.replace(
            "  return {",
            "  try { if (condition) return {}; } catch {}\n  return {",
        ),
        provider.replace(
            "  return {",
            "  try { initialize(); } catch { return {}; }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  try { initialize(); } finally { return {}; }\n  return {",
        ),
        provider.replace("  return {", "  if (condition) throw failure;\n  return {"),
        provider.replace(
            "  return {",
            "  initialization: { break initialization; }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  while (condition) { if (skip) continue; break; }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  switch (condition) { default: initialize(); }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  condition ? initialize() : other();\n  return {",
        ),
        provider.replace(
            "  return {",
            "  try { initialize(); } finally { toggleSidebar = other; }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  function replaceCaptured() { [toggleSidebar] = callbacks; }\n  return {",
        ),
        provider.replace(
            "  return {",
            "  function deleteCaptured() { delete registry.toggleSidebar; }\n  return {",
        ),
        provider.replace("\n});", "\n  initialize();\n});"),
    ];

    let mut batch_paths = Vec::new();
    for (index, initialization) in initializations.iter().enumerate() {
        let initialized = provider
            .replace("  return {", &format!("{initialization}  return {{"))
            .replacen(
                r#"export const useUiStore = defineStore("ui""#,
                &format!(r#"export const useInit{index}Store = defineStore("init{index}""#),
                1,
            );
        let store = format!("stores/init{index}.ts");
        let consumer = format!("app/Init{index}.vue");
        w.write(&store, &initialized);
        w.write(
            &consumer,
            &format!(
                "<script setup lang=\"ts\">\nconst uiStore = useInit{index}Store();\nuiStore.toggleSidebar();\n</script>\n"
            ),
        );
        batch_paths.push(store);
        batch_paths.push(consumer);
    }
    for (index, _) in unsupported_bodies.iter().enumerate() {
        let source = provider.replacen(
            r#"export const useUiStore = defineStore("ui""#,
            &format!(r#"export const useBad{index}Store = defineStore("bad{index}""#),
            1,
        );
        let store = format!("stores/bad{index}.ts");
        let consumer = format!("app/Bad{index}.vue");
        w.write(&store, &source);
        w.write(
            &consumer,
            &format!(
                "<script setup lang=\"ts\">\nconst uiStore = useBad{index}Store();\nuiStore.toggleSidebar();\n</script>\n"
            ),
        );
        batch_paths.push(store);
        batch_paths.push(consumer);
    }

    let delta_paths: Vec<&str> = batch_paths.iter().map(String::as_str).collect();
    let batch_delta = w.delta(&delta_paths);
    assert_eq!(
        batch_delta["changed_files"],
        2 * (initializations.len() + unsupported_bodies.len())
    );
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    for index in 0..initializations.len() {
        let consumer = format!("app/Init{index}.vue");
        consumer_status(&parity.incremental_connection, &consumer, "resolved");
    }
    for index in 0..unsupported_bodies.len() {
        let consumer = format!("app/Bad{index}.vue");
        consumer_status(&parity.incremental_connection, &consumer, "resolved");
    }

    // Mutate only the providers to their unsupported variants, leaving consumers untouched to test existing-provider invalidation.
    let mut mutate_paths = Vec::new();
    for (index, unsupported) in unsupported_bodies.iter().enumerate() {
        let source = unsupported.replacen(
            r#"export const useUiStore = defineStore("ui""#,
            &format!(r#"export const useBad{index}Store = defineStore("bad{index}""#),
            1,
        );
        let store = format!("stores/bad{index}.ts");
        w.write(&store, &source);
        mutate_paths.push(store);
    }
    let mutate_refs: Vec<&str> = mutate_paths.iter().map(String::as_str).collect();
    let mutate_delta = w.delta(&mutate_refs);
    assert_eq!(mutate_delta["changed_files"], unsupported_bodies.len());
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    for index in 0..unsupported_bodies.len() {
        let consumer = format!("app/Bad{index}.vue");
        consumer_status(&parity.incremental_connection, &consumer, "unresolved");
        let conn = &parity.incremental_connection;
        let calls: i64 = crate::languages::harness::query_scalar(
            conn,
            "SELECT count(*) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)=?1 AND kind='calls'",
            [&consumer],
        );
        assert_eq!(
            calls, 0,
            "{consumer} should have 0 calls after provider invalidation"
        );
    }
    w.write("stores/duplicate.ts", provider);
    w.delta(&["stores/duplicate.ts"]);
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    {
        let conn = &parity.incremental_connection;
        let statuses: Vec<String> = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/Configured.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='uiStore.toggleSidebar'").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            statuses,
            ["ambiguous"],
            "duplicate exported factories cannot choose a provider"
        );
    }
    w.remove("stores/duplicate.ts");
    w.delta(&["stores/duplicate.ts"]);
    let parity = cold_references.observe(&w, w.open(), crate::languages::support::relation_rows);
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
    observe(&parity.incremental_connection, true, true);
}
