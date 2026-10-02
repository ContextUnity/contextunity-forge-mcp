use super::{json, Client, Value, Workspace};

#[test]
fn stdio_ast_pages_do_not_repeat_or_skip_matches() {
    let workspace = Workspace::new();
    workspace.write(
        "service.py",
        &(0..75).map(|i| format!("print({i})\n")).collect::<String>(),
    );
    std::fs::create_dir_all(workspace.0.join("scope")).unwrap();
    workspace.write("scope/a.py", "def target():\n    return 1\n");
    workspace.write(
        "scope/b.py",
        "def target():\n    return 1\ndef other():\n    return 1\n",
    );
    workspace.write("outside.py", "def target():\n    return 1\n");
    #[cfg(feature = "lang-rust")]
    workspace.write("scope/same.rs", "fn target() -> i32 { 1 }\n");
    let mut client = Client::new(&workspace);
    let mut offset = 0;
    let mut generation = Value::Null;
    let mut lines = Vec::new();
    loop {
        let payload = client.payload("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","limit":30,"offset":offset,"generation":generation}));
        let page = &payload["matches"];
        let items = page["items"].as_array().unwrap();
        lines.extend(items.iter().map(|item| item["line"].as_u64().unwrap()));
        if page["has_more"] == false {
            assert_eq!(page["total"], 75);
            break;
        }
        offset = page["next_offset"].as_u64().unwrap();
        generation = page["generation"].clone();
    }
    assert_eq!(lines, (1..=75).collect::<Vec<_>>());

    let mut paths = Vec::new();
    let mut offset = 0;
    let mut generation = Value::Null;
    loop {
        let payload = client.payload("ast_grep_search", json!({"pattern":"def target():\n    return 1","language":"python","path":"scope","limit":1,"offset":offset,"generation":generation}));
        let page = &payload["matches"];
        paths.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["path"].as_str().unwrap().to_owned()),
        );
        if page["has_more"] == false {
            assert_eq!(page["total"], 2);
            break;
        }
        offset = page["next_offset"].as_u64().unwrap();
        generation = page["generation"].clone();
    }
    assert_eq!(paths, ["scope/a.py", "scope/b.py"]);
    let captured = client.payload("ast_grep_search", json!({"pattern":"def $NAME():\n    return 1","language":"python","path":"scope/b.py","limit":100}));
    assert_eq!(captured["matches"]["total"], 2);
    assert_eq!(captured["matches"]["items"].as_array().unwrap().len(), 2);
}
