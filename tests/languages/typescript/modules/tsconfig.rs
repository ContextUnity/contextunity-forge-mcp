use super::*;
#[test]
fn tsconfig_extends_inherits_alias_origin_and_rebuilds_on_base_change() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "configs/tsconfig.base.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["../shared/*"]}}}"#,
        ),
        ("configs/tsconfig.app.json", r#"{"extends":"./tsconfig.base.json"}"#),
        (
            "apps/one/tsconfig.json",
            r#"{"extends":"../../configs/tsconfig.app.json"}"#,
        ),
        ("shared/helper.ts", "export function helper() {}"),
        ("shared/alternate/helper.ts", "export function helper() {}"),
        (
            "apps/one/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
        (
            "apps/two/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["src/*"]}}}"#,
        ),
        ("apps/two/src/helper.ts", "export function helper() {}"),
        (
            "apps/two/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
        (
            "apps/three/tsconfig.json",
            r#"{"extends":"../../configs/tsconfig.app.json","compilerOptions":{"baseUrl":"../../../outside","paths":{"@shared/*":["src/*"]}}}"#,
        ),
        (
            "apps/three/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let targets = |path: &str| persisted_call_paths(&workspace.open(), path);
    assert_eq!(targets("apps/one/src/main.ts"), ["shared/helper.ts"]);
    assert_eq!(targets("apps/two/src/main.ts"), ["apps/two/src/helper.ts"]);
    let invalid = workspace.open();
    let statuses: Vec<String> = invalid.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='apps/three/src/main.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='helper'")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert!(!statuses.is_empty(), "invalid config must retain visible coverage");
    assert!(
        statuses
            .iter()
            .all(|status| matches!(status.as_str(), "external" | "unresolved")),
        "escaping baseUrl cannot inherit a local provider: {statuses:?}"
    );
    drop(invalid);
    workspace.write(
        "configs/tsconfig.base.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["../shared/alternate/*"]}}}"#,
    );
    workspace.delta(&["configs/tsconfig.base.json"]);
    assert_eq!(targets("apps/one/src/main.ts"), ["shared/alternate/helper.ts"]);
    assert_eq!(targets("apps/two/src/main.ts"), ["apps/two/src/helper.ts"]);
    let conn = workspace.open();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn tsconfig_paths_choose_first_indexed_alternative_and_relink_on_new_file() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "apps/one/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["missing/*","src/*"]}}}"#,
        ),
        ("apps/one/src/thing.ts", "export function thing() {}"),
        ("apps/two/missing/thing.ts", "export function thing() {}"),
        ("apps/one/src/main.ts", "import { thing } from '@lib/thing'; thing();"),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let targets = || persisted_call_paths(&workspace.open(), "apps/one/src/main.ts");
    assert_eq!(targets(), ["apps/one/src/thing.ts"]);
    workspace.write("apps/one/missing/thing.ts", "export function thing() {}");
    workspace.delta(&["apps/one/missing/thing.ts"]);
    assert_eq!(targets(), ["apps/one/missing/thing.ts"]);
    let conn = workspace.open();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn type_only_imports_resolve_indexed_local_interface_provider() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        ("app/types.ts", "export interface ColumnStateItem { colId: string }"),
        (
            "app/consumer.ts",
            "import type { ColumnStateItem } from './types';\nexport function accept(value: ColumnStateItem): void {}\nColumnStateItem();",
        ),
        ("other/types.ts", "export interface ColumnStateItem { decoy: boolean }"),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let statuses: Vec<String> = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/consumer.ts' AND line <= 2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ColumnStateItem' ORDER BY line")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert!(!statuses.is_empty(), "type-only import must have persisted coverage");
    assert!(
        statuses.iter().all(|status| status == "resolved"),
        "type-only import statuses: {statuses:?}"
    );
    let targets: Vec<String> = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/consumer.ts' AND e.kind='imports' AND dst.name='ColumnStateItem' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(targets, ["app/types.ts"]);
    let invalid_call: String = query_scalar(
        &conn,
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/consumer.ts' AND line=3 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ColumnStateItem'",
        [],
    );
    assert_eq!(invalid_call, "unresolved");
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn exported_enum_members_require_exact_indexed_provider() {
    let files = [
        (
            "app/colors.ts",
            "export enum Color { Red, Blue = 2 } const local = Color.Blue;",
        ),
        (
            "app/use.ts",
            "import { Color } from './colors'; const selected = Color.Red; const missing = Color.Green; function shadow(Color) { return Color.Red; }",
        ),
        ("app/call.ts", "import { Color } from './colors';\nColor.Red();"),
        (
            "app/type_use.ts",
            "import type { Color } from './colors';\ntype Choice = Color;\nconst invalid = Color.Red;\nconst bare = Color;\nfunction consume(value) {} consume(Color);",
        ),
        ("other/colors.ts", "export enum Color { Green }"),
    ];
    let facts: BTreeMap<String, Facts> = extracted_files(&files, Some("typescript"));
    let provider = &facts["app/colors.ts"];
    assert!(
        provider
            .nodes
            .iter()
            .any(|node| node.kind == "enum" && node.name == "Color"),
        "{:#?}",
        provider.nodes
    );
    for name in ["Red", "Blue"] {
        assert!(
            provider.nodes.iter().any(|node| node.kind == "field"
                && node.name == name
                && node.qualname.ends_with(&format!("Color.{name}"))),
            "{name}: {:#?}",
            provider.nodes
        );
    }
    let graph = ExtractorHarness::link_graph(&facts);
    for (path, expression, status) in [
        ("app/colors.ts", "Color.Blue", "resolved"),
        ("app/use.ts", "Color.Red", "resolved"),
        ("app/use.ts", "Color.Green", "unresolved"),
        ("app/use.ts", "Color.Red", "unresolved"),
        ("app/type_use.ts", "Color.Red", "unresolved"),
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.path == path && row.expression == expression && row.status == status),
            "{path}: {expression}: expected {status}, got {:#?}",
            graph.coverage
        );
    }
    for line in [4, 5] {
        let runtime: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.path == "app/type_use.ts" && row.line == line && row.expression == "Color")
            .collect();
        assert!(
            !runtime.is_empty() && runtime.iter().all(|row| row.status == "unresolved"),
            "{runtime:#?}"
        );
        assert!(!graph.edges.iter().any(|edge| {
            edge.path == "app/type_use.ts" && edge.line == line && edge.kind == "references" && edge.evidence == "Color"
        }));
    }
    let selected: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.path == "app/use.ts" && edge.kind == "references" && edge.evidence == "Color.Red")
        .collect();
    assert_eq!(selected.len(), 1, "{selected:#?}");
    assert!(
        provider
            .nodes
            .iter()
            .any(|node| node.id == selected[0].dst && node.name == "Red")
    );
    let callable: Vec<_> = graph
        .coverage
        .iter()
        .filter(|row| row.path == "app/call.ts" && row.line == 2 && row.expression == "Color.Red")
        .collect();
    assert!(
        !callable.is_empty() && callable.iter().all(|row| row.status == "unresolved"),
        "{callable:#?}"
    );
    assert!(
        !graph
            .edges
            .iter()
            .any(|edge| { edge.path == "app/call.ts" && edge.line == 2 && edge.kind == "calls" })
    );

    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in files {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let mut stmt = conn.prepare("SELECT line,status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/type_use.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Color' AND line IN (4,5) ORDER BY line").unwrap();
    let persisted: Vec<(usize, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for line in [4, 5] {
        let rows: Vec<_> = persisted.iter().filter(|(at, _)| *at == line).collect();
        assert!(
            !rows.is_empty() && rows.iter().all(|(_, status)| status == "unresolved"),
            "{persisted:?}"
        );
    }
    let runtime_edges: usize = query_scalar(
        &conn,
        "SELECT COUNT(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/type_use.ts' AND e.line IN (4,5) AND e.kind='references'",
        [],
    );
    assert_eq!(runtime_edges, 0);
    let mut callable_stmt = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/call.ts' AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Color.Red'").unwrap();
    let callable_rows: Vec<String> = callable_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        !callable_rows.is_empty() && callable_rows.iter().all(|status| status == "unresolved"),
        "{callable_rows:?}"
    );
    let callable_edges: usize = query_scalar(
        &conn,
        "SELECT COUNT(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/call.ts' AND e.line=2 AND e.kind='calls'",
        [],
    );
    assert_eq!(callable_edges, 0);
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(stmt);
    drop(callable_stmt);
    drop(conn);
}

#[test]
fn typescript_export_type_reexport_import_targets() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("types.ts", "export type Foo = { id: string };\n");
    workspace.write("generated.ts", "export interface JsonValue { value: string; }\n");
    workspace.write(
        "barrel.ts",
        r#"
import type { JsonValue } from "./generated";
export type { JsonValue };
export type { Foo } from "./types";
export type { Outside } from "outside-package";
"#,
    );
    workspace.write(
        "consumer.ts",
        r#"
import type { Foo, JsonValue, Outside } from "./barrel";
function test(f: Foo, j: JsonValue, o: Outside) {}
"#,
    );
    workspace.build();
    let conn = workspace.open();

    let coverage = |path: &str, expression: &str| -> String {
        query_scalar(
            &conn,
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
        )
    };

    assert_eq!(coverage("consumer.ts", "Foo"), "resolved");
    assert_eq!(coverage("consumer.ts", "JsonValue"), "resolved");
    assert_eq!(coverage("consumer.ts", "Outside"), "unresolved");

    let edge_dst = |path: &str, expression: &str| -> String {
        query_scalar(
            &conn,
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)=?2",
            rusqlite::params![path, expression],
        )
    };
    assert_eq!(edge_dst("consumer.ts", "Foo"), "types.ts");
    assert_eq!(edge_dst("consumer.ts", "JsonValue"), "generated.ts");

    drop(conn);
}

#[test]
fn export_type_keeps_classes_and_value_export_drops_type_aliases() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "box.ts",
        "export class Box { id = 1; }\nexport enum Color { Red = 1 }\n",
    );
    workspace.write("barrel.ts", "export type { Box, Color } from \"./box\";\n");
    workspace.write("only.ts", "export type Foo = { id: string };\nexport { Foo };\n");
    workspace.write(
        "consumer.ts",
        "import type { Box, Color } from \"./barrel\";\nfunction use(box: Box, color: Color) {}\n",
    );
    workspace.write("use.js", "import { Foo } from \"./only\";\nFoo();\n");
    workspace.build();
    let conn = workspace.open();
    let coverage = |expression: &str| -> String {
        query_scalar(
            &conn,
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
        )
    };
    assert_eq!(coverage("Box"), "resolved");
    assert_eq!(coverage("Color"), "resolved");
    let foo: String = query_scalar(
        &conn,
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='use.js' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Foo'",
        [],
    );
    assert_eq!(foo, "unresolved");
    let box_dst: String = query_scalar(
        &conn,
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.ts' AND e.kind='references' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='Box'",
        [],
    );
    assert_eq!(box_dst, "box.ts");
    drop(conn);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_typescript_mjs_cjs_and_package_exports() {
    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write(
        "packages/math/package.json",
        r#"{
  "name": "@acme/math",
  "exports": {
    ".": "./src/index.mjs",
    "./calculator": "./src/calc.cjs"
  }
}
"#,
    );
    w.write(
        "packages/math/src/index.mjs",
        r#"export function add(a, b) {
    return a + b;
}
"#,
    );
    w.write(
        "packages/math/src/calc.cjs",
        r#"exports.multiply = function(a, b) {
    return a * b;
};
"#,
    );
    w.write(
        "packages/app/src/main.ts",
        r#"import { add } from "@acme/math";
import { multiply } from "@acme/math/calculator";

export function run() {
    return add(2, 3);
}
"#,
    );

    w.build();
    let conn = w.open();

    // Verify .mjs and .cjs were scanned and extracted
    let mjs_count: i64 = query_scalar(&conn, "SELECT count(*) FROM files WHERE path LIKE '%.mjs'", []);
    assert_eq!(mjs_count, 1);

    let cjs_count: i64 = query_scalar(&conn, "SELECT count(*) FROM files WHERE path LIKE '%.cjs'", []);
    assert_eq!(cjs_count, 1);

    // Verify import edges resolved via package.json exports
    let import_edges: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='imports'").unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(
        import_edges.iter().any(|(_, dst)| dst.contains("index.mjs")),
        "expected import edge to index.mjs: {:?}",
        import_edges
    );
    assert!(
        import_edges.iter().any(|(_, dst)| dst.contains("calc.cjs")),
        "expected import edge to calc.cjs: {:?}",
        import_edges
    );
}
