use super::*;

#[test]
fn milestone_and_plan_directories_configured_and_excluded_from_scanner() {
    let root = MilestoneHarness::from_catalog(include_str!(
        "../../fixtures/milestones/configured_directories.json"
    ));
    std::fs::write(
        root.root().join(".git/info/exclude"),
        ".forge/\ncode_map.sqlite*\n",
    )
    .unwrap();
    let paths = contextunity_forge_mcp::engine::scanner::scan(root.root(), None)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    for path in [
        "src/lib.rs",
        "docs/architecture.md",
        "extensions/commerce/src/models.py",
        "extensions/commerce/docs/README.md",
        "docs/milestones/README.md",
        "docs/plans/README.md",
        "extensions/commerce/docs/milestones/README.md",
        "extensions/commerce/docs/plans/README.md",
        "docs/plans/platform_plan.md",
        "docs/milestones/010-platform.md",
        "docs/plans/archive/README.md",
        "docs/milestones/archive/README.md",
        "extensions/commerce/docs/plans/commerce_plan.md",
        "extensions/commerce/docs/milestones/010-commerce.md",
    ] {
        assert!(
            paths.iter().any(|entry| entry.as_str() == path),
            "missing scanner entry: {path}"
        );
    }

    let index_path = root.path("code_map.sqlite");
    root.build_to(&index_path);
    let conn = contextunity_forge_mcp::db::reader::open(&index_path, root.root()).unwrap();
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(10),
        0,
        None,
        None,
    )
    .unwrap();
    for (query, path, visible) in CONFIGURED_NAVIGATION {
        let paths = contextunity_forge_mcp::db::reader::search_docs_with_options(
            &conn,
            query,
            &contextunity_forge_mcp::db::reader::DocSearchOptions {
                doc_type: None,
                component: None,
                include_excerpt: false,
                page: &page,
            },
        )
        .unwrap()["sections"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(paths.contains(&path.to_owned()), visible, "{query}/{path}");
    }

    let listed = root.list(false, None, 10);
    let milestones = listed["milestones"].as_array().unwrap();
    assert_eq!(milestones.len(), 2);
    let ids: Vec<_> = milestones
        .iter()
        .map(|milestone| milestone["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"m-platform"));
    assert!(ids.contains(&"m-commerce"));
    let show_commerce = root.show_result("m-commerce", false).unwrap();
    assert_eq!(show_commerce["id"], "m-commerce");
    assert_eq!(show_commerce["title"], "Commerce Milestone");

    let init_result = root
        .init(milestone_engine::Init {
            slug: Some("commerce-v2".into()),
            plan: Some("extensions/commerce/docs/plans/commerce_plan.md".into()),
            ..milestone_init_options()
        })
        .unwrap();
    assert_eq!(init_result["id"], "m-commerce-v2");
    assert_eq!(
        init_result["path"],
        "extensions/commerce/docs/milestones/020-commerce-v2.md"
    );
    let init_v2_content = root.document("extensions/commerce/docs/milestones/020-commerce-v2.md");
    assert!(init_v2_content.contains("project: extensions.commerce"));

    root.commit_fixture_inputs(&["forge-mcp.yaml", "src", "docs", "extensions"]);
    let synced = root
        .sync("extensions/commerce/docs/milestones/010-commerce.md")
        .unwrap();
    let tasks_array = synced["tasks"].as_array().unwrap();
    assert_eq!(tasks_array.len(), 1);
    let task_id = tasks_array[0]["task_id"].as_str().unwrap();
    assert!(task_id.starts_with("extensions.commerce/"));

    let listed_after_sync = root.list(false, None, 10);
    let commerce_m = listed_after_sync["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|milestone| milestone["id"] == "m-commerce")
        .unwrap();
    assert_eq!(commerce_m["tasks"][0]["status"], "ready");

    let store = root.store_in("forge-mcp", "extensions.commerce");
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    root.complete_task(CompletionMode::Store {
        store: &store,
        task_id: &task.task_id,
    })
    .unwrap();
    drop(store);

    let commit = root.head();
    let evidence =
        root.default_fixture_evidence("m-commerce", "pytest extensions/commerce/tests", 5, 0);
    let handoff_res = root
        .handoff_at("m-commerce", Some(&commit), &evidence)
        .unwrap();
    assert_eq!(handoff_res["id"], "m-commerce");
    assert!(root
        .root()
        .join("extensions/commerce/docs/milestones/archive/010-commerce.md")
        .exists());
    assert!(!root
        .root()
        .join("extensions/commerce/docs/milestones/010-commerce.md")
        .exists());

    for (slug, title, dir) in [
        ("escape", "Escape", "../outside"),
        ("unconfigured", "Unconfigured", "unconfigured/milestones"),
    ] {
        let result = root.init(milestone_engine::Init {
            slug: Some(slug.into()),
            title: Some(title.into()),
            dir: Some(dir.into()),
            ..milestone_init_options()
        });
        assert!(result.is_err(), "init accepted {dir:?}");
    }
}

const CONFIGURED_NAVIGATION: [(&str, &str, bool); 10] = [
    (
        "rootmilestoneoverviewtoken",
        "docs/milestones/README.md",
        true,
    ),
    ("rootplanoverviewtoken", "docs/plans/README.md", true),
    (
        "commercemilestoneoverviewtoken",
        "extensions/commerce/docs/milestones/README.md",
        true,
    ),
    (
        "commerceplanoverviewtoken",
        "extensions/commerce/docs/plans/README.md",
        true,
    ),
    (
        "rootmilestonecontractisolationtoken",
        "docs/milestones/010-platform.md",
        true,
    ),
    (
        "rootplancontractisolationtoken",
        "docs/plans/platform_plan.md",
        true,
    ),
    (
        "commercemilestonecontractisolationtoken",
        "extensions/commerce/docs/milestones/010-commerce.md",
        true,
    ),
    (
        "commerceplancontractisolationtoken",
        "extensions/commerce/docs/plans/commerce_plan.md",
        true,
    ),
    (
        "archivemilestoneoverviewtoken",
        "docs/milestones/archive/README.md",
        false,
    ),
    (
        "archiveplanoverviewtoken",
        "docs/plans/archive/README.md",
        false,
    ),
];
