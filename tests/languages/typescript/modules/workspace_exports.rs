use super::*;
#[test]
fn typescript_package_exports_paths_respect_workspace_boundaries() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "package.json",
            r#"{"private":true,"workspaces":["components/*","apps/*"]}"#,
        ),
        (
            "components/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":{"import":"./src/entry.ts"}}}"#,
        ),
        ("components/tool/src/entry.ts", "export function shared() {}"),
        (
            "packages/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/decoy.ts"}}"#,
        ),
        ("packages/tool/src/decoy.ts", "export function shared() {}"),
        (
            "apps/one/package.json",
            r#"{"name":"one","dependencies":{"@suite/tool":"workspace:*"}}"#,
        ),
        (
            "apps/one/tsconfig.json",
            r#"{
          // Paths are local to this app.
          "compilerOptions": {"baseUrl": ".", "paths": {"@local/*": ["src/*",],},},
        }"#,
        ),
        ("apps/one/src/helper.ts", "export function helper() {}"),
        ("apps/one/alternative/helper.ts", "export function helper() {}"),
        (
            "apps/one/src/main.ts",
            "import { helper } from '@local/helper'; import { shared } from '@suite/tool/entry'; helper(); shared();",
        ),
        (
            "apps/two/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["src/*"]}}}"#,
        ),
        ("apps/two/src/helper.ts", "export function helper() {}"),
        (
            "apps/two/src/main.ts",
            "import { helper } from '@local/helper'; helper();",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let call_targets = |path: &str| persisted_call_paths(&conn, path);
    assert_eq!(
        call_targets("apps/one/src/main.ts"),
        ["apps/one/src/helper.ts", "components/tool/src/entry.ts",]
    );
    assert_eq!(
        call_targets("apps/two/src/main.ts"),
        ["apps/two/src/helper.ts"]
    );
    drop(conn);
    workspace.write(
        "apps/one/tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["alternative/*"]}}}"#,
    );
    workspace.delta(&["apps/one/tsconfig.json"]);
    let delta = workspace.open();
    contextunity_forge_mcp::core::commitments::verify(&delta).unwrap();
    let changed_targets = persisted_call_paths(&delta, "apps/one/src/main.ts");
    assert_eq!(
        changed_targets,
        [
            "apps/one/alternative/helper.ts",
            "components/tool/src/entry.ts"
        ]
    );
    drop(delta);
}

#[test]
fn package_export_wildcards_select_declared_workspace_and_specific_subpath() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "package.json",
            r#"{"private":true,"workspaces":["components/*","apps/*"]}"#,
        ),
        (
            "components/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./features/*":"./src/features/*.ts","./features/special/*":{"import":"./src/special/*.ts"}}}"#,
        ),
        ("components/tool/src/features/helper.ts", "export function feature() {}"),
        (
            "components/tool/src/features/special/action.ts",
            "export function special() {}",
        ),
        ("components/tool/src/special/action.ts", "export function special() {}"),
        (
            "packages/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./features/*":"./src/decoy/*.ts"}}"#,
        ),
        ("packages/tool/src/decoy/helper.ts", "export function feature() {}"),
        (
            "apps/one/src/main.ts",
            "import { feature } from '@suite/tool/features/helper'; import { special } from '@suite/tool/features/special/action'; feature(); special();",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let targets = persisted_call_paths(&conn, "apps/one/src/main.ts");
    assert_eq!(
        targets,
        [
            "components/tool/src/features/helper.ts",
            "components/tool/src/special/action.ts"
        ]
    );
    drop(conn);
}

#[test]
fn pnpm_workspace_globs_select_declared_package_exports_and_delta() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "package.json",
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - 'components/*'\n  - 'apps/*'\n  - '!components/excluded'\n",
        ),
        (
            "components/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/entry.ts"}}"#,
        ),
        (
            "components/tool/src/entry.ts",
            "export function shared() {}",
        ),
        (
            "components/excluded/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/excluded.ts"}}"#,
        ),
        (
            "components/excluded/src/excluded.ts",
            "export function shared() {}",
        ),
        (
            "packages/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/decoy.ts"}}"#,
        ),
        ("packages/tool/src/decoy.ts", "export function shared() {}"),
        (
            "apps/one/package.json",
            r#"{"name":"one","dependencies":{"@suite/tool":"workspace:*"}}"#,
        ),
        (
            "apps/one/src/main.ts",
            "import { shared } from '@suite/tool/entry'; shared();",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let targets = || persisted_call_paths(&workspace.open(), "apps/one/src/main.ts");
    assert_eq!(targets(), ["components/tool/src/entry.ts"]);
    workspace.write(
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n  - 'apps/*'\n",
    );
    workspace.delta(&["pnpm-workspace.yaml"]);
    assert_eq!(targets(), ["packages/tool/src/decoy.ts"]);
    let conn = workspace.open();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}
