use super::super::*;

#[test]
fn nuxt_autoimports_require_declared_project_and_preserve_local_shadow() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        (
            "forge-mcp.yaml",
            "roots: [src, composables, nuxt/src, nuxt/composables, plain/src, nuxt.config.ts, nuxt/nuxt.config.ts]\ndocs: []\n",
        ),
        (
            "package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "src/auto.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
        ("nuxt.config.ts", "export default defineNuxtConfig({});"),
        (
            "composables/useHttpClient.ts",
            "export function useHttpClient() { return { ok: true }; }",
        ),
        ("src/client.ts", "useHttpClient();"),
        (
            "src/imported-client.ts",
            "import { useHttpClient as request } from '~/composables/useHttpClient';\ntype RequestResult = ReturnType<typeof request>;\nrequest();\n",
        ),
        (
            "src/relative-client.ts",
            "import { useHttpClient as client } from '../composables/useHttpClient';\ntype ClientResult = ReturnType<typeof client>;\nclient();\n",
        ),
        (
            "src/namespace-query.ts",
            "import { useHttpClient as selected } from '../composables/useHttpClient';\ntype Mixed = [typeof selected, selected];\ntype Nested = { value: ReturnType<typeof selected> };\n",
        ),
        (
            "src/type-only-query.ts",
            "import type { useHttpClient as onlyType } from '../composables/useHttpClient';\ntype Only = typeof onlyType;\n",
        ),
        (
            "src/local-client.ts",
            "function useHttpClient() {} useHttpClient();",
        ),
        (
            "nuxt/package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "nuxt/nuxt.config.ts",
            "export default defineNuxtConfig({});",
        ),
        (
            "nuxt/src/auto.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
        (
            "nuxt/src/local.ts",
            "function computed() {} function ref() {} computed(); ref();",
        ),
        (
            "nuxt/composables/useHttpClient.ts",
            "export function useHttpClient() { return { nested: true }; }",
        ),
        ("nuxt/src/client.ts", "useHttpClient();"),
        ("plain/package.json", r#"{"dependencies":{"vue":"3.5.0"}}"#),
        ("plain/src/client.ts", "useHttpClient();"),
        (
            "plain/src/plain.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |path: &str, expression: &str| -> Vec<(String, String)> {
        let mut stmt = conn.prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2").unwrap();
        let rows = stmt
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    for expression in ["computed", "ref"] {
        let root = coverage("src/auto.ts", expression);
        assert!(
            root.iter().any(|(status, evidence)| status == "external"
                && evidence.contains("builtin:nuxt_autoimport")),
            "root package.json outside source roots must activate Nuxt {expression}: {root:?}"
        );
        let auto = coverage("nuxt/src/auto.ts", expression);
        assert!(
            auto.iter().any(|(status, evidence)| status == "external"
                && evidence.contains("builtin:nuxt_autoimport")),
            "Nuxt {expression}: {auto:?}"
        );
        assert!(
            coverage("nuxt/src/local.ts", expression)
                .iter()
                .any(|(status, _)| status == "resolved"),
            "local {expression}"
        );
        let plain = coverage("plain/src/plain.ts", expression);
        assert!(
            !plain.is_empty() && plain.iter().all(|(status, _)| status == "unresolved"),
            "plain {expression}: {plain:?}"
        );
    }
    let callable_edges = |path: &str, line: usize, kind: &str, provider: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE p.path=?1 AND e.line=?2 AND e.kind=?3 AND dp.path=?4 AND d.name='useHttpClient' AND d.kind='function'",
            rusqlite::params![path, line, kind, provider],
            |row| row.get(0),
        )
        .unwrap()
    };
    let mut callable_failures = Vec::new();
    // These are ordinary legacy package/config and import facts. All three
    // missing contours must fail at runtime, not at a future-schema parser.
    for (path, expression, line, kind, provider) in [
        (
            "src/client.ts",
            "useHttpClient",
            1,
            "calls",
            "composables/useHttpClient.ts",
        ),
        (
            "nuxt/src/client.ts",
            "useHttpClient",
            1,
            "calls",
            "nuxt/composables/useHttpClient.ts",
        ),
        (
            "src/imported-client.ts",
            "request",
            2,
            "references",
            "composables/useHttpClient.ts",
        ),
        (
            "src/imported-client.ts",
            "request",
            3,
            "calls",
            "composables/useHttpClient.ts",
        ),
        (
            "src/relative-client.ts",
            "client",
            2,
            "references",
            "composables/useHttpClient.ts",
        ),
        (
            "src/relative-client.ts",
            "client",
            3,
            "calls",
            "composables/useHttpClient.ts",
        ),
    ] {
        let statuses: Vec<String> = conn
            .prepare("SELECT c.status FROM resolution_coverage c JOIN path_dictionary p USING(path_id) JOIN coverage_expressions x USING(expression_id) WHERE p.path=?1 AND c.line=?2 AND x.expression=?3 ORDER BY c.status")
            .unwrap()
            .query_map(rusqlite::params![path, line, expression], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let exact = callable_edges(path, line, kind, provider);
        if statuses.is_empty() || statuses.iter().any(|status| status != "resolved") || exact != 1 {
            callable_failures.push(format!(
                "{path}:{line} {expression} {kind} -> {provider}: statuses={statuses:?}, exact={exact}"
            ));
        }
    }
    let plain = coverage("plain/src/client.ts", "useHttpClient");
    assert!(
        !plain.is_empty() && plain.iter().all(|(status, _)| status == "unresolved"),
        "unconfigured provider must remain unresolved: {plain:?}"
    );
    assert_eq!(
        callable_edges(
            "plain/src/client.ts",
            1,
            "calls",
            "composables/useHttpClient.ts"
        ),
        0
    );
    let local = coverage("src/local-client.ts", "useHttpClient");
    assert!(
        !local.is_empty() && local.iter().all(|(status, _)| status == "resolved"),
        "local declaration must keep ordinary precedence: {local:?}"
    );
    assert_eq!(
        callable_edges(
            "src/local-client.ts",
            1,
            "calls",
            "composables/useHttpClient.ts"
        ),
        0
    );
    let query_states = |path: &str, line: usize, expression: &str| -> Vec<String> {
        conn.prepare("SELECT DISTINCT c.status FROM resolution_coverage c JOIN path_dictionary p USING(path_id) JOIN coverage_expressions x USING(expression_id) WHERE p.path=?1 AND c.line=?2 AND x.expression=?3 ORDER BY c.status")
            .unwrap().query_map(rusqlite::params![path,line,expression], |row|row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(
        query_states("src/namespace-query.ts", 2, "selected"),
        ["resolved", "unresolved"],
        "same-owner value query must not authorize the ordinary type identifier"
    );
    assert_eq!(
        query_states("src/namespace-query.ts", 3, "selected"),
        ["resolved"],
        "nested imported value query"
    );
    for line in [2, 3] {
        assert_eq!(
            callable_edges(
                "src/namespace-query.ts",
                line,
                "references",
                "composables/useHttpClient.ts"
            ),
            1,
            "one exact actual function per proven value query"
        );
    }
    assert_eq!(
        query_states("src/type-only-query.ts", 2, "onlyType"),
        ["unresolved"],
        "type-only import supplies no value callable"
    );
    assert_eq!(
        callable_edges(
            "src/type-only-query.ts",
            2,
            "references",
            "composables/useHttpClient.ts"
        ),
        0
    );
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path='src/namespace-query.ts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let facts = serde_json::to_value(reader::decode_local_facts(&blob).unwrap()).unwrap();
    let references: Vec<&serde_json::Value> = facts["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| {
            reference["expression"] == "selected" && reference["kind"] == "references"
        })
        .collect();
    let markers: Vec<(&serde_json::Value, &serde_json::Value)> = facts["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|node| {
            node["details"]["value_flow"]["annotation_references"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|fact| fact["expression"] == "selected" && fact["value_query"] == true)
                .map(move |fact| (node, fact))
        })
        .collect();
    assert_eq!(
        markers.len(),
        2,
        "only the two actual typeof operands carry value evidence"
    );
    let mut marker_positions: Vec<(u64, u64)> = markers
        .iter()
        .map(|(_, marker)| {
            (
                marker["position"]["line"].as_u64().unwrap(),
                marker["position"]["column"].as_u64().unwrap(),
            )
        })
        .collect();
    marker_positions.sort_unstable();
    assert_eq!(
        marker_positions,
        [
            (2, "type Mixed = [typeof ".len() as u64),
            (3, "type Nested = { value: ReturnType<typeof ".len() as u64),
        ],
        "only literal AST typeof operands receive exact-position evidence"
    );
    for (owner, marker) in markers {
        assert_eq!(
            references
                .iter()
                .filter(|reference| reference["source"] == owner["id"]
                    && reference["line"] == marker["position"]["line"]
                    && reference["column"] == marker["position"]["column"])
                .count(),
            1,
            "typed marker matches exactly one existing reference and owner"
        );
    }
    let mixed: Vec<&serde_json::Value> = references
        .iter()
        .copied()
        .filter(|reference| reference["line"] == 2)
        .collect();
    assert_eq!(
        mixed.len(),
        2,
        "both same-line namespace occurrences remain distinct references"
    );
    assert_eq!(
        mixed[0]["source"], mixed[1]["source"],
        "same owner is intentional"
    );
    assert_ne!(
        mixed[0]["column"], mixed[1]["column"],
        "no declaration-line/column-zero collapse"
    );
    let mut mixed_columns: Vec<u64> = mixed
        .iter()
        .map(|reference| reference["column"].as_u64().unwrap())
        .collect();
    mixed_columns.sort_unstable();
    assert_eq!(
        mixed_columns,
        [0, "type Mixed = [typeof ".len() as u64],
        "ordinary type reference keeps legacy coordinates; only typeof is exact"
    );
    let legacy: contextunity_forge_mcp::core::semantic::AnnotationReferenceFact =
        serde_json::from_value(
            serde_json::json!({"expression":"selected","position":{"line":2,"column":21}}),
        )
        .unwrap();
    assert!(
        !legacy.value_query,
        "legacy annotation evidence is type namespace only"
    );
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("value_query")
            .is_none(),
        "false marker keeps legacy wire omission"
    );
    assert!(serde_json::from_value::<contextunity_forge_mcp::core::semantic::AnnotationReferenceFact>(serde_json::json!({"expression":"selected","position":{"line":2,"column":21},"value_query":"true"})).is_err(),"malformed marker cannot supply authority");
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    assert!(
        callable_failures.is_empty(),
        "scoped exported-callable/root-alias/typeof target losses: {callable_failures:#?}"
    );
    drop(conn);
}
