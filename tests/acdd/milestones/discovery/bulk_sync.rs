use super::*;

const INVALID_CONTRACT_HEADERS: [&str; 4] = [
    "no frontmatter\n",
    "---\nid: [broken\n---\n",
    "---\ntitle: Missing identity\n---\n",
    "---\nid: 17\n---\n",
];

#[test]
fn bulk_sync_rejects_invalid_numbered_manifests_in_custom_directories() {
    const DIRECT_PATH: &str = "contracts/001-fixture.md";
    const NESTED_PATH: &str = "contracts/nested/002-fixture.md";
    const ARCHIVE_PATH: &str = "contracts/archive/003-fixture.md";
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\nmilestones: [contracts]\ntask_repository: fixture\ntask_project: fixture\n",
    );
    root.write("src/lib.rs", "pub fn fixture() {}\n");
    root.write("contracts/README.md", "# Contracts\n");
    let fixture = include_str!("../../fixtures/milestones/custom_contract.md");
    let nested_fixture = fixture.replace("m-fixture", "m-nested");
    let archived_fixture = fixture.replace("m-fixture", "m-archived");
    let invoke = |reference: Option<&str>| {
        root.sync_selection(reference, reference.is_none().then_some("fixture"))
    };

    for (path, valid, task_id) in [
        (DIRECT_PATH, fixture, "fixture/m-fixture:one"),
        (NESTED_PATH, nested_fixture.as_str(), "fixture/m-nested:one"),
    ] {
        root.write(path, valid);
        let synced = invoke(None).unwrap();
        assert_eq!(synced["total"], 1);
        assert_eq!(list_workspaces(root.root()).unwrap(), vec!["fixture"]);
        let (_, body) = valid.split_once("\n---\n").unwrap();
        let before = root
            .store_in("fixture", "fixture")
            .inspect_details(task_id)
            .unwrap();
        for header in INVALID_CONTRACT_HEADERS {
            root.write(path, &format!("{header}\n{body}"));
            for reference in [None, Some(path)] {
                let output = invoke(reference);
                assert!(
                    output.is_err(),
                    "sync {reference:?} accepted {header:?}: {output:?}"
                );
                assert!(
                    format!("{:#}", output.as_ref().unwrap_err()).contains(path),
                    "diagnostic omits {path}: {output:?}"
                );
            }
            let after = root
                .store_in("fixture", "fixture")
                .inspect_details(task_id)
                .unwrap();
            assert_eq!(before, after, "invalid input changed persisted task state");
            root.write(path, valid);
        }
        std::fs::remove_file(root.root().join(path)).unwrap();
    }

    root.write(ARCHIVE_PATH, &archived_fixture);
    assert_eq!(invoke(None).unwrap()["total"], 0);
    let (_, archived_body) = archived_fixture.split_once("\n---\n").unwrap();
    for header in INVALID_CONTRACT_HEADERS {
        root.write(ARCHIVE_PATH, &format!("{header}\n{archived_body}"));
        let skipped = invoke(None).unwrap();
        assert_eq!(skipped["total"], 0, "bulk sync must exclude archives");
        let explicit = invoke(Some(ARCHIVE_PATH)).unwrap_err();
        assert!(format!("{explicit:#}").contains(ARCHIVE_PATH));
        root.write(ARCHIVE_PATH, &archived_fixture);
    }
}
