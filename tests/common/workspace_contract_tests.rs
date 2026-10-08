#[test]
fn workspace_builds_and_applies_delta_in_its_owned_temp_directory() {
    let workspace = crate::common::Workspace::new();
    let root = workspace.root().to_path_buf();
    workspace.write("src/lib.rs", "pub fn before() {}\n");
    workspace.build();

    let before = workspace.open();
    let before_count: i64 = before
        .query_row(
            "SELECT count(*) FROM nodes WHERE name='before'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_count, 1);
    drop(before);

    workspace.write("src/lib.rs", "pub fn after() {}\n");
    workspace.delta(&["src/lib.rs"]);
    let after = workspace.open();
    let after_names: Vec<String> = after
        .prepare("SELECT name FROM nodes WHERE kind='function' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(after_names, ["after"]);
    drop(after);

    drop(workspace);
    assert!(
        !root.exists(),
        "temporary workspace should be removed on drop"
    );
}
