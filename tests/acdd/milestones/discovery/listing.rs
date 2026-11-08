use super::*;

#[test]
fn milestone_list_and_show_report_scoped_documents_and_sqlite_progress() {
    let root = MilestoneHarness::from_catalog(include_str!(
        "../../fixtures/milestones/progress_catalog.json"
    ));
    let mut store = root.store();
    for path in [
        "docs/milestones/010-queued.md",
        "docs/milestones/011-live.md",
        "docs/milestones/archive/020-prior.md",
    ] {
        store
            .sync(
                &Milestone::parse(&root.document(path), "forge-mcp").unwrap(),
                path,
                root.root(),
            )
            .unwrap();
    }
    root.write(
        "docs/milestones/010-queued.md",
        include_str!("../../fixtures/milestones/queued_after_sync.md"),
    );
    root.write(
        "docs/milestones/archive/020-prior.md",
        include_str!("../../fixtures/milestones/archived_after_sync.md"),
    );
    for task_ref in ["first", "shipped"] {
        let mut task = store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .find(|task| task.task_id.ends_with(&format!(":{task_ref}")))
            .unwrap();
        task.status = "completed".into();
        MilestoneHarness::persist_descriptor(&store, &task);
    }
    let listed = root.list(false, None, 10);
    let rows = listed["milestones"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let markdown = contextunity_forge_mcp::core::format::render(
        &listed,
        contextunity_forge_mcp::core::format::OutputFormat::Markdown,
        "milestone_list",
    );
    for text in [
        "## Active Milestones",
        "## Planned Milestones",
        "Live work",
        "Queued work",
        "docs/milestones/011-live.md",
        "1/2",
        "milestone show",
        "task list",
        "--archive",
        "--status",
    ] {
        assert!(markdown.contains(text), "missing {text:?}: {markdown}");
    }
    assert!(
        !markdown.contains('|'),
        "pipe tables are not milestone cards"
    );
    assert!(
        markdown.contains("](file://"),
        "milestone paths must be clickable"
    );
    let queued_row = rows.iter().find(|row| row["id"] == "m-queued").unwrap();
    assert_eq!(
        (
            queued_row["title"].as_str(),
            queued_row["status"].as_str(),
            queued_row["started_at"].as_str(),
            queued_row["completion"].as_str()
        ),
        (Some("Queued work"), Some("planned"), Some("-"), Some("0/2"))
    );
    assert!(queued_row["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "file-only" && task["status"] == "unsynced"));
    let live = rows.iter().find(|row| row["id"] == "m-live").unwrap();
    assert_eq!(
        live.as_object().unwrap().len(),
        10,
        "preserve the JSON row shape"
    );
    assert_eq!(
        (live["status"].as_str(), live["completion"].as_str()),
        (Some("active"), Some("1/2"))
    );
    assert_eq!(live["tasks"].as_array().unwrap().len(), 2);
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["status"] == "completed"));
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "second" && task["status"] == "ready"));
    for (archive, status) in [(true, None), (false, Some("all"))] {
        assert_eq!(
            root.list(archive, status, 10)["milestones"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    assert_eq!(
        root.list(true, None, 0)["milestones"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "archive limits must not hide active or planned milestones"
    );
    let completed = root.list(false, Some("completed"), 10);
    assert_eq!(completed["milestones"][0]["id"], "m-prior");
    assert_eq!(completed["milestones"][0]["completion"], "1/1");

    let broken_archive = MilestoneFixtureBuilder::new("m-broken")
        .title("Broken archive")
        .status("completed")
        .without_started_at()
        .task("invalid", &["src/"])
        .build()
        .replacen("task_ref: invalid\n", "task_ref: [invalid\n", 1);
    assert!(broken_archive.contains("task_ref: [invalid\n"));
    root.write("docs/milestones/archive/030-broken.md", &broken_archive);
    assert_eq!(
        root.list(false, None, 10)["milestones"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(root.show("011", false)["id"], "m-live");

    let shown = root.show("m-live", false);
    assert!(shown.get("full_document").is_none());
    assert_eq!(
        (
            shown["id"].as_str(),
            shown["title"].as_str(),
            shown["status"].as_str()
        ),
        (Some("m-live"), Some("Live work"), Some("active"))
    );
    assert_eq!(shown["frontmatter"]["id"], "m-live");
    assert_eq!(shown["depends_on"][0], "m-queued");
    assert_eq!(shown["invariants"][0], "Keep receipts");
    assert!(shown["outcome"]
        .as_str()
        .unwrap()
        .contains("Deliver visible progress."));
    assert_eq!(shown["tasks"].as_array().unwrap().len(), 2);
    assert!(shown["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["target"] == "First result"));
    let by_prefix = root.show("011", true);
    assert!(by_prefix.get("full_document").is_some());
    assert_eq!(by_prefix["id"], shown["id"]);
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Implementation steps for first."));
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Delivery notes for second."));
    let by_path = root.show("docs/milestones/011-live.md", false);
    assert_eq!(by_path["id"], shown["id"]);
    assert_eq!(root.show("020", false)["status"], "completed");

    let active = root.document("docs/milestones/011-live.md");
    let archived = root.document("docs/milestones/archive/020-prior.md");
    root.write(
        "docs/milestones/032-duplicate.md",
        &active
            .replace("m-live", "m-duplicate")
            .replace("Live work", "Duplicate active"),
    );
    root.write(
        "docs/milestones/archive/032-duplicate.md",
        &archived
            .replace("m-prior", "m-duplicate-archived")
            .replace("Prior work", "Duplicate archived"),
    );
    assert_eq!(root.show("032", false)["id"], "m-duplicate");
    assert_eq!(
        root.show("docs/milestones/archive/032-duplicate.md", false)["id"],
        "m-duplicate-archived"
    );

    let override_source = MilestoneFixtureBuilder::new("m-override")
        .title("Override work")
        .without_started_at()
        .repository("alternate")
        .project("special")
        .task("delivered", &["src/"])
        .task_target("Delivered override")
        .build();
    root.write("docs/milestones/025-override.md", &override_source);
    let mut alternate = root.store_in("alternate", "special");
    alternate
        .sync(
            &Milestone::parse_with_identity(&override_source, "alternate", "special").unwrap(),
            "docs/milestones/025-override.md",
            root.root(),
        )
        .unwrap();
    let mut delivered = alternate.list(None, "all", None).unwrap().pop().unwrap();
    delivered.status = "completed".into();
    MilestoneHarness::persist_descriptor(&alternate, &delivered);
    let overridden = root.list(false, None, 10);
    let override_row = overridden["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "m-override")
        .unwrap();
    assert_eq!(override_row["completion"], "1/1");
    assert_eq!(override_row["tasks"][0]["status"], "completed");
    let invalid_frontmatter = MilestoneFixtureBuilder::new("m-invalid")
        .title("Invalid frontmatter")
        .without_status()
        .build()
        .replacen("doc_type: contract\n", "doc_type: contract\nbroken: [\n", 1);
    assert!(invalid_frontmatter.contains("broken: [\n"));
    root.write(
        "docs/milestones/031-malformed-frontmatter.md",
        &invalid_frontmatter,
    );
    let invalid_id = root.show_result("m-does-not-exist", false).unwrap_err();
    let invalid_id_output = format!("{invalid_id:#}");
    assert!(
        invalid_id_output.contains("failed to read milestone id from"),
        "milestone ID lookup must preserve frontmatter parse errors: {invalid_id_output}"
    );
}

#[test]
fn archived_milestone_metrics_and_limits_survive_unreadable_sqlite() {
    let history = MilestoneHarness::configured_workspace();
    history.write(".forge/tasks.sqlite", "unreadable SQLite sentinel");
    for index in 0..102 {
        let source = include_str!("../../fixtures/milestones/archive_metrics.md")
            .replace("INDEX", &format!("{index:03}"))
            .replace("NUMBER", &index.to_string());
        history.write(
            format!("docs/milestones/archive/{index:03}-history.md"),
            &source,
        );
    }
    for index in 0..102 {
        assert!(
            history
                .document(&format!("docs/milestones/archive/{index:03}-history.md"))
                .contains("completion: 99/99"),
            "forged stored completion for archive {index}"
        );
    }
    for (limit, expected) in [
        (None, 10),
        (Some("0"), 1),
        (Some("1"), 1),
        (Some("100"), 100),
        (Some("101"), 100),
    ] {
        let value = history.list(true, None, limit.unwrap_or("10").parse().unwrap());
        assert_eq!(value["milestones"].as_array().unwrap().len(), expected);
        assert_eq!(value["archive_total"], 102);
        assert_eq!(value["archive_displayed"], expected);
        assert_eq!(value["milestones"][0]["completion"], "1/3");
        assert_eq!(value["milestones"][0]["tasks"][1]["status"], "cancelled");
    }
    assert_eq!(
        history.file_bytes(".forge/tasks.sqlite"),
        b"unreadable SQLite sentinel"
    );
    let value = history.list(true, None, 1);
    let markdown = contextunity_forge_mcp::core::format::render(
        &value,
        contextunity_forge_mcp::core::format::OutputFormat::Markdown,
        "milestone_list",
    );
    assert!(markdown.contains("Archive: 1 of 102 documents displayed."));
    assert!(markdown.contains("## Completed Milestones"));
    assert!(markdown.contains("## Active Milestones\n\nNone."));
    assert!(markdown.contains("## Planned Milestones\n\nNone."));
    assert!(!markdown.contains('|'));
    let filtered = history.list(false, Some("completed"), 1);
    assert_eq!(filtered["archive_total"], 102);
    assert_eq!(filtered["milestones"].as_array().unwrap().len(), 1);
}

#[test]
fn milestone_list_renders_untrusted_manifest_text_literally() {
    let hostile = MilestoneHarness::configured_workspace();
    let filename = "docs/milestones/011-[docs]`code`_emphasis_<html>&.md";
    let title = "Literal [link](target) `code` _emphasis_ <html>&\n\n## Injected";
    let hostile_manifest = MilestoneFixtureBuilder::new("m-literal")
        .title(title)
        .started_at("first\n\n## Injected date")
        .build();
    hostile.write(filename, &hostile_manifest);
    let literal_json = hostile.list(false, None, 10);
    let markdown = contextunity_forge_mcp::core::format::render(
        &literal_json,
        contextunity_forge_mcp::core::format::OutputFormat::Markdown,
        "milestone_list",
    );
    assert!(
        !markdown.contains("\n## Injected"),
        "manifest text must not create blocks: {markdown}"
    );
    for literal in [
        r"Literal \[link\]\(target\) \`code\` \_emphasis\_ \<html\>\&",
        r"[docs/milestones/011-\[docs\]\`code\`\_emphasis\_\<html\>\&.md](file://",
        "%5Bdocs%5D%60code%60_emphasis_%3Chtml%3E%26.md)",
        "- Started: `first ## Injected date`",
    ] {
        assert!(
            markdown.contains(literal),
            "missing literal {literal:?}: {markdown}"
        );
    }
    assert_eq!(
        literal_json["milestones"][0]["title"], title,
        "only presentation may normalize text"
    );
    assert_eq!(literal_json["documents"][0]["path"], filename);
}
