use super::super::*;

#[test]
fn ignored_root_ancestor_and_linked_manifests_do_not_activate_nuxt() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    let linked = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [src, nuxt.config.ts, nested/src, nested/nuxt.config.ts]\ndocs: []\nignore: [package.json]\nlinked_workspaces:\n  - name: shared\n    path: '{}'\n    roots: [src, nuxt.config.ts]\n    ignore: [package.json]\n",
            linked.0.display()
        ),
    );
    for (path, source) in [
        (
            "package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        ("nuxt.config.ts", "export default defineNuxtConfig({});"),
        (
            "src/root.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
        (
            "nested/package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "nested/nuxt.config.ts",
            "export default defineNuxtConfig({});",
        ),
        (
            "nested/src/ancestor.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
    ] {
        workspace.write(path, source);
    }
    for (path, source) in [
        (
            "package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        ("nuxt.config.ts", "export default defineNuxtConfig({});"),
        (
            "src/linked.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
    ] {
        linked.write(path, source);
    }

    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |path: &str, expression: &str| -> Vec<String> {
        let mut stmt = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2").unwrap();
        stmt.query_map(rusqlite::params![path, expression], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    for (path, expression) in [
        ("src/root.ts", "computed"),
        ("src/root.ts", "ref"),
        ("nested/src/ancestor.ts", "computed"),
        ("nested/src/ancestor.ts", "ref"),
        ("[shared]/src/linked.ts", "computed"),
        ("[shared]/src/linked.ts", "ref"),
    ] {
        let rows = coverage(path, expression);
        assert!(
            !rows.is_empty() && rows.iter().all(|status| status == "unresolved"),
            "ignored root, ancestor, and linked package manifests must not activate Nuxt: {path} {expression} -> {rows:?}"
        );
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);

    // Unimported names cannot acquire authority from directory or spelling conventions,
    // even when exactly one unrelated provider exists in an indexed workspace.
    workspace.write("src/unconfigured.ts", "export function foreign() { return useForeign(); }\nexport function neighbor() { return useNeighbor(); }\nexport function plain() { return usePlain(); }\nexport function store() { return remoteStore(); }\n");
    workspace.write(
        "src/stores/plain.ts",
        "export function usePlain() { return 1; }\n",
    );
    workspace.write(
        "nested/src/composables/neighbor.ts",
        "export function useNeighbor() { return 1; }\n",
    );
    linked.write(
        "src/stores/foreign.ts",
        "export function useForeign() { return 1; }\nexport function remoteStore() { return 2; }\n",
    );
    workspace.write("src/explicit.ts", "import {usePlain} from './stores/plain';\nexport function imported() { return usePlain(); }\n");
    let consumer = fs::read(workspace.join("src/unconfigured.ts")).unwrap();
    let assert_unconfigured = || {
        let conn = workspace.open();
        contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
        for expression in ["useForeign", "useNeighbor", "usePlain", "remoteStore"] {
            let states: Vec<String> = conn.prepare("SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='src/unconfigured.ts' AND x.expression=?1 ORDER BY c.line").unwrap().query_map([expression], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(
                states,
                ["unresolved"],
                "unconfigured {expression} never uses an unrelated provider"
            );
        }
        let guessed: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='src/unconfigured.ts' AND e.kind='calls'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            guessed, 0,
            "unconfigured conventions produce no exact calls"
        );
        let canonical: String = conn.query_row("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='src/stores/plain.ts' AND n.name='usePlain'", [], |row| row.get(0)).unwrap();
        let targets: Vec<String> = conn.prepare("SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='src/explicit.ts' AND s.name='imported' AND e.kind='calls' ORDER BY d.id").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            targets,
            [canonical],
            "ordinary explicit imports keep their canonical target"
        );
        assert_eq!(
            fs::read(workspace.join("src/unconfigured.ts")).unwrap(),
            consumer,
            "provider/rule-only changes leave the consumer untouched"
        );
    };
    // A real, nonempty disabled override exercises effective rules rather than an empty pack.
    for disabled in [false, true, false] {
        let manifest = ".forge/frameworks/nuxt.toml";
        if disabled {
            workspace.write(
                manifest,
                &format!(
                    "enabled = false\n{}",
                    include_str!("../../../../../src/engine/languages/manifests/nuxt.toml")
                ),
            );
        } else if workspace.join(manifest).exists() {
            fs::remove_file(workspace.join(manifest)).unwrap();
        }
        writer::build(&workspace, &database, None).unwrap();
        assert_unconfigured();
        workspace.assert_cold_equivalent();
        for provider in [
            Some("export function usePlain() { return 1; }\n"),
            None,
            Some("export function usePlain() { return 2; }\n"),
        ] {
            let path = "src/utils/alternate.ts";
            if let Some(source) = provider {
                workspace.write(path, source);
            } else {
                fs::remove_file(workspace.join(path)).unwrap();
            }
            workspace.delta(path);
            assert_unconfigured();
            workspace.assert_cold_equivalent();
        }
        fs::remove_file(workspace.join("src/utils/alternate.ts")).unwrap();
        workspace.delta("src/utils/alternate.ts");
        assert_unconfigured();
        workspace.assert_cold_equivalent();
    }
}

#[test]
fn nuxt_components_import_links_tsx_to_vue_only_for_declared_nuxt() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        (
            "nuxt/package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "nuxt/nuxt.config.ts",
            "export default defineNuxtConfig({});",
        ),
        (
            "nuxt/components/base/WidgetCard.vue",
            "<template><article>Card</article></template>",
        ),
        (
            "nuxt/src/page.tsx",
            "import { BaseWidgetCard } from '#components';\n",
        ),
        ("plain/package.json", r#"{"dependencies":{"vue":"3.5.0"}}"#),
        (
            "plain/components/base/WidgetCard.vue",
            "<template><article>Card</article></template>",
        ),
        (
            "plain/src/page.vue",
            "<template><BaseWidgetCard /></template>",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let statuses = |path: &str| -> Vec<String> {
        conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='BaseWidgetCard' ORDER BY line,status")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let nuxt_import_targets: Vec<String> = conn
        .prepare("SELECT DISTINCT target_path.path FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE owner.path='nuxt/src/page.tsx' AND e.kind='imports' ORDER BY target_path.path")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let nuxt_statuses = statuses("nuxt/src/page.tsx");
    let plain_statuses = statuses("plain/src/page.vue");
    let expected_target = ["nuxt/components/base/WidgetCard.vue"];
    assert!(
        nuxt_statuses.iter().any(|status| status == "resolved")
            && nuxt_import_targets == expected_target
            && plain_statuses == ["unresolved"],
        "#components must resolve through the declared Nuxt linker while a plain Vue tag stays unresolved; nuxt statuses={nuxt_statuses:?}, import targets={nuxt_import_targets:?}, plain statuses={plain_statuses:?}"
    );
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}
