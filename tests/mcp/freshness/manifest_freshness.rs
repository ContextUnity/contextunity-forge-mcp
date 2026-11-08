use super::*;

fn discovery_package_fixture() -> anyhow::Result<(Workspace, Workspace, McpTestHarness)> {
    let linked_packages = Workspace::with_files(&[(
        "weird+%#1/pyproject.toml",
        "[project]\nname = 'linked-manifest-package'\n",
    )]);
    let packages = Workspace::with_files(&[
        (
            "rust/Cargo.toml",
            "[package]\nname = 'manifest-cargo-package'\nversion = '0.1.0'\ndescription = 'Declared crate description'\n",
        ),
        (
            "python/pyproject.toml",
            "[project]\nname = 'manifest-python-package'\nversion = '0.1.0'\ndescription = 'Declared Python description'\n",
        ),
        (
            "poetry/pyproject.toml",
            "[tool.poetry]\nname = 'manifest-poetry-package'\nversion = '0.1.0'\ndescription = 'Declared Poetry description'\n",
        ),
        (
            "javascript/package.json",
            "{\"name\":\"manifest-npm-workspace\",\"workspaces\":[\"packages/*\"]}",
        ),
        (
            "javascript/packages/child/package.json",
            "{\"name\":\"manifest-npm-child\",\"description\":\"Declared npm description\"}",
        ),
        (
            "pnpm/package.json",
            "{\"name\":\"manifest-pnpm-workspace\"}",
        ),
        ("pnpm/pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
        (
            "pnpm/packages/child/package.json",
            "{\"name\":\"manifest-pnpm-child\"}",
        ),
        (
            "yarn/package.json",
            "{\"name\":\"manifest-yarn-workspace\",\"workspaces\":[\"packages/*\"]}",
        ),
        ("yarn/yarn.lock", "# yarn lockfile v1\n"),
        (
            "yarn/packages/child/package.json",
            "{\"name\":\"manifest-yarn-child\"}",
        ),
        (
            "go/go.mod",
            "module example.org/manifest-go-module\n\ngo 1.22\n",
        ),
        ("directory-only/unit.py", "def directory_marker(): pass\n"),
        ("broken/Cargo.toml", "this is not valid TOML = [\n"),
        (
            "unsupported/pyproject.toml",
            "[tool.black]\nline-length = 88\n",
        ),
        (
            "configured-ignored/Cargo.toml",
            "[package]\nname='configured-ignored-package'\nversion='0.1.0'\n",
        ),
        (
            "gitignored/Cargo.toml",
            "[package]\nname='gitignored-package'\nversion='0.1.0'\n",
        ),
        (
            "docs/guide.md",
            "---\ntitle: Discovery documentation hub\ndoc_type: guide\n---\n# Discovery documentation hub\nDeclared guide.\n",
        ),
        (
            "docs/milestones/010-discovery.md",
            "---\nid: m-discovery-fixture\ntitle: Discovery active milestone\ndoc_type: contract\nstatus: active\n---\n# Discovery active milestone\n",
        ),
        (".gitignore", "gitignored/\n"),
    ]);
    packages.write(
        "forge-mcp.yaml",
        &format!(
            "milestones: [docs/milestones]\nignore: [configured-ignored]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [.]\n",
            linked_packages.root().display()
        ),
    );
    drop(packages.build());
    let client = McpTestHarness::new(&packages)?;
    Ok((packages, linked_packages, client))
}

#[test]
fn same_server_refreshes_non_source_manifests_outside_source_roots() {
    let ws = Workspace::with_files(&[
        ("forge-mcp.yaml", "roots: [src]\n"),
        (
            "src/consumer.py",
            "import novel_library\ndef run(): return novel_library.fetch()\n",
        ),
    ]);
    let server = in_process_server(&ws);
    let evidence = || {
        server.read(|conn| Ok(json!({
        "evidence": conn.query_row("SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='novel_library'", [], |row| row.get::<_, String>(0))?,
        "manifest_digest": conn.query_row("SELECT value FROM metadata WHERE key='manifest_digest'", [], |row| row.get::<_, String>(0))?,
    }))).unwrap()
    };
    let before = evidence();
    assert!(!before["evidence"].as_str().unwrap().contains("manifest"));
    ws.write("requirements-dev.txt", "novel_library>=1\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = evidence();
    assert!(after["evidence"].as_str().unwrap().contains("manifest"));
    assert_ne!(before["manifest_digest"], after["manifest_digest"]);
    assert_eq!(after["freshness"]["refresh"], "rebuild");
    fs::remove_file(ws.path("requirements-dev.txt")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let removed = evidence();
    assert_eq!(removed["manifest_digest"], before["manifest_digest"]);
    assert!(!removed["evidence"].as_str().unwrap().contains("manifest"));
    let mut missing = Vec::new();
    let (packages, linked_packages, mut client) =
        discovery_package_fixture().expect("manifest discovery fixture");
    let hub = client
        .payload("get_doc", json!({"path_or_id":"docs/guide.md"}))
        .expect("MCP tool should succeed");
    let hub_line = hub["sections"]["items"][0]["line"].as_u64().unwrap();
    let overview = client
        .payload("code_map_overview", json!({"limit":100}))
        .expect("MCP tool should succeed");
    assert_eq!(
        overview["discovery"]["active_milestones"]["availability"],
        "available"
    );
    let milestone = overview["discovery"]["active_milestones"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|milestone| milestone["id"] == "m-discovery-fixture")
        .expect("canonical active milestone row");
    assert_eq!(milestone["status"], "active");
    assert_eq!(milestone["completion"], "0/0");
    let (_, response) = client
        .call(
            "code_map_overview",
            serde_json::json!({"limit":100,"format":"markdown"}),
        )
        .expect("MCP transport");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let package_markdown = response["result"]["content"][0]["text"].as_str().unwrap();
    for expected in [
        "manifest-cargo-package",
        "manifest-python-package",
        "manifest-poetry-package",
        "manifest-npm-workspace",
        "manifest-npm-child",
        "manifest-pnpm-workspace",
        "manifest-pnpm-child",
        "manifest-yarn-workspace",
        "manifest-yarn-child",
        "example.org/manifest-go-module",
        "linked-manifest-package",
        "Discovery documentation hub",
        "Discovery active milestone",
        "m-discovery-fixture",
        "0/0 complete",
    ] {
        if !package_markdown.contains(expected) {
            missing.push(format!(
                "overview must supply actual admitted manifest/hub identity {expected:?}"
            ));
        }
    }
    for (path, line) in [("docs/guide.md", Some(hub_line)), ("rust", None)] {
        let coordinate = format!(
            "file://{}{}",
            packages.root().join(path).display(),
            line.map_or_else(String::new, |line| format!("#L{line}"))
        );
        if !package_markdown.contains(&coordinate) {
            missing.push(format!(
                "overview must resolve the actual coordinate {coordinate}"
            ));
        }
    }
    assert!(!package_markdown.contains("configured-ignored-package"));
    assert!(!package_markdown.contains("gitignored-package"));
    let directory_section = package_markdown
        .split("## Directory components")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("overview directory components section");
    assert!(directory_section.contains("directory-only"));
    let manifest_section = package_markdown
        .split("## Manifest components")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("overview manifest components section");
    assert!(!manifest_section.contains("directory-only"));
    assert!(!package_markdown.contains("directory_marker"));
    assert!(
        package_markdown.contains("malformed_manifest"),
        "{package_markdown}"
    );
    assert!(
        package_markdown.contains("unsupported_manifest_shape"),
        "{package_markdown}"
    );
    assert!(
        package_markdown.contains(&format!(
            "file://{}/weird%2B%25%231/pyproject.toml",
            linked_packages.root().display()
        )),
        "linked manifest URI must resolve against its owning root: {package_markdown}"
    );
    let full_manifest_page = &overview["discovery"]["manifest_components"];
    let linked_manifest = full_manifest_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "linked-manifest-package")
        .expect("linked package identity comes from its admitted manifest");
    assert_eq!(linked_manifest["workspace"], "library");
    assert_eq!(linked_manifest["root"], "weird+%#1");
    assert_eq!(linked_manifest["manifest_path"], "weird+%#1/pyproject.toml");

    let (_, scoped_overview_response) = client
        .call(
            "code_map_overview",
            serde_json::json!({"aspects":["manifest_components"],"limit":1,"format":"markdown"}),
        )
        .expect("MCP transport");
    assert_ne!(scoped_overview_response["result"]["isError"], true);
    let scoped_overview = scoped_overview_response["result"]["content"][0]["text"]
        .as_str()
        .expect("aspect-scoped overview Markdown");
    let manifest_continuation = scoped_overview
        .lines()
        .find(|line| line.starts_with("Continue manifest components "))
        .expect("bounded manifest page continuation");
    assert!(
        manifest_continuation.contains("aspects=[\"manifest_components\"]")
            && manifest_continuation
                .contains(&format!("workspace_root={}", packages.root().display())),
        "overview continuation must retain its aspect selection and workspace: {manifest_continuation}"
    );

    let first_page = client
        .payload("code_map_overview", json!({"limit":1}))
        .expect("MCP tool should succeed");
    let components = &first_page["discovery"]["manifest_components"];
    assert_eq!(components["availability"], "available");
    assert_eq!(components["items"].as_array().unwrap().len(), 1);
    assert_eq!(components["limit"], 1);
    assert!(components["has_more"].as_bool().unwrap());
    assert_eq!(components["next_offset"], 1);
    assert_eq!(components["diagnostics_available"], true);
    assert_eq!(components["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(components["diagnostic_total"], 2);
    assert_eq!(components["diagnostics_truncated"], true);
    let next_page = client
        .payload(
            "code_map_overview",
            json!({"limit":1,"offset":1,"generation":components["generation"]}),
        )
        .expect("MCP tool should succeed");
    let next_components = &next_page["discovery"]["manifest_components"];
    assert_eq!(next_components["total"], components["total"]);
    assert_eq!(next_components["offset"], 1);
    assert_ne!(next_components["items"][0], components["items"][0]);

    let (_bounded, bounded_client) = McpTestHarness::with_files(&[
        ("forge-mcp.yaml", "roots: [src]\nlimits:\n  max_files: 1\n"),
        ("src/app.py", "def app(): pass\n"),
        ("src/a/package.json", "{\"name\":\"bounded-a\"}"),
        ("src/b/package.json", "{\"name\":\"bounded-b\"}"),
    ]);
    let mut bounded_client = bounded_client.expect("MCP initialize");
    let bounded_overview = bounded_client
        .payload("code_map_overview", json!({"limit":10}))
        .expect("MCP tool should succeed");
    assert_eq!(
        bounded_overview["discovery"]["manifest_components"]["availability"],
        "unavailable"
    );
    assert_eq!(
        bounded_overview["discovery"]["manifest_components"]["cause"],
        "manifest_file_count_limit"
    );
    assert!(
        missing.is_empty(),
        "discovery composition contract: {}\n\n{package_markdown}",
        missing.join("\n"),
    );
    bounded_client
        .finish()
        .expect("server must finish successfully");
    client.finish().expect("server must finish successfully");
}

#[test]
fn manifest_changes_trigger_deterministic_rebuild_in_both_admit_and_delta() {
    let ws = Workspace::with_files(&[
        ("forge-mcp.yaml", "roots: [src]\n"),
        ("requirements.txt", "requests>=2.0\n"),
        ("unindexed/requirements.txt", "unindexed_library>=1\n"),
        ("src/app.py", "import requests\n"),
    ]);

    let db = ws.path(".forge/index.sqlite");
    ws.build_to(&db);
    let initial_digest = crate::sqlite_fixture::manifest_digest_at(&db).unwrap();

    let server = in_process_server(&ws);
    let unchanged = server.read(|_| Ok(json!({}))).unwrap();
    assert_eq!(unchanged["freshness"]["refresh"], "none");
    ws.write("unindexed/requirements.txt", "unindexed_library>=2\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let outside_roots = server.read(|_| Ok(json!({}))).unwrap();
    assert_eq!(outside_roots["freshness"]["refresh"], "none");

    // 1. Change manifest on disk and verify Server::admit performs rebuild
    ws.write("requirements.txt", "requests>=2.1\n");
    let server = in_process_server(&ws);
    let after_admit = server
        .read(|c| {
            Ok(json!({
                "manifest_digest": c.query_row("SELECT value FROM metadata WHERE key='manifest_digest'", [], |row| row.get::<_, String>(0))?
            }))
        })
        .unwrap();
    let admit_digest = after_admit["manifest_digest"].as_str().unwrap();
    assert_ne!(admit_digest, initial_digest);

    // 2. Change manifest on disk and verify delta performs rebuild
    ws.write("requirements.txt", "requests>=2.2\n");
    let delta_res = contextunity_forge_mcp::engine::pipeline::delta_with_cache_dir(
        ws.root(),
        &db,
        &[PathBuf::from("src/app.py")],
        &ws.path(".forge/extraction-cache"),
    )
    .unwrap();
    assert!(delta_res.is_object());
    let after_delta = crate::sqlite_fixture::manifest_digest_at(&db).unwrap();
    assert_ne!(after_delta, admit_digest);
    assert_ne!(after_delta, initial_digest);
}
