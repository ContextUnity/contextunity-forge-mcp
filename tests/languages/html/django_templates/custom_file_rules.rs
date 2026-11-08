use super::*;

#[test]
fn custom_file_rules() {
    // One persisted observation helper keeps every incremental state under
    // the same validation and edge assertions.
    for extension in ["toml", "yaml", "yml"] {
        let workspace = HtmlWorkspaceHarness::new();
        workspace.write("pyproject.toml", "[project]\nname='custom-file-rules'\n");
        workspace.write("app/views.py", "from workshop.renderer import emit as paint\n\ndef page(request):\n    return paint('catalog/card.html', request)\n");
        const TARGET: &str = "app/views/catalog/card.html";
        workspace.write(TARGET, "<p>Custom</p>\n");
        let path = format!(".forge/frameworks/previously_unknown.{extension}");
        let mut pack = serde_json::json!({
            "semantic_rules":[{"id":"custom-file","namespace":"custom","scope":"project","activation":[],"operation":{"kind":"call_to_file","language":"python","callee":{"kind":"imported","module":"workshop.renderer","export":"emit"},"argument":0,"target":{"kind":"project_path_suffix","prefix":"views/","language":"html"},"relation":"renders_template"}}]
        });
        let write_pack = |value: &serde_json::Value| {
            let source = if extension == "toml" {
                toml::to_string(value).unwrap()
            } else {
                serde_yaml::to_string(value).unwrap()
            };
            workspace.write(&path, &source);
        };
        let assert_edges = |source_path: &str, target: Option<&str>| {
            let observed = workspace.file_edge_observations(source_path);
            observed.validity.assert_valid();
            if let Some(target) = target {
                assert_eq!(
                    observed.page_ids.len(),
                    1,
                    "unique persisted page node at {source_path}: {:?}",
                    observed.page_ids
                );
                assert_eq!(
                    observed.edges,
                    [(observed.page_ids[0].clone(), format!("module:{target}"), 4)],
                    "persisted file edge at {source_path}"
                );
            } else {
                assert!(
                    observed.edges.is_empty(),
                    "unexpected persisted file edges at {source_path}: {:?}",
                    observed.edges
                );
            }
        };

        write_pack(&pack);
        workspace.build();
        assert_edges("app/views.py", Some(TARGET));
        if extension != "toml" {
            continue;
        }

        const OVERLAP: &str = "app/views/views/views/catalog/card.html";
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/views/".into();
        workspace.write(OVERLAP, "<p>Overlapping normal segments</p>\n");
        write_pack(&pack);
        workspace.delta(&[&path, OVERLAP]);
        workspace.assert_cold_parity();
        assert_edges("app/views.py", Some(OVERLAP));

        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        workspace.remove_file(OVERLAP).unwrap();
        write_pack(&pack);
        workspace.delta(&[&path, OVERLAP]);
        assert_edges("app/views.py", Some(TARGET));

        pack["semantic_rules"][0]["namespace"] = "renamed".into();
        write_pack(&pack);
        workspace.delta(&[&path]);
        assert_edges("app/views.py", Some(TARGET));

        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "missing/".into();
        write_pack(&pack);
        workspace.delta(&[&path]);
        assert_edges("app/views.py", None);

        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        pack["semantic_rules"][0]["activation"] =
            serde_json::json!([{"kind":"package","name":"workshop"}]);
        write_pack(&pack);
        workspace.delta(&[&path]);
        assert_edges("app/views.py", None);
        workspace.write(
            "pyproject.toml",
            "[project]\nname='custom-file-rules'\ndependencies=['workshop']\n",
        );
        workspace.delta(&["pyproject.toml"]);
        assert_edges("app/views.py", Some(TARGET));

        let mut independent = pack.clone();
        independent["semantic_rules"][0]["id"] = "independent".into();
        independent["semantic_rules"][0]["namespace"] = "independent".into();
        independent["semantic_rules"][0]["operation"]["callee"]["module"] =
            "independent.renderer".into();
        workspace.write(
            ".forge/frameworks/another.toml",
            &toml::to_string(&independent).unwrap(),
        );
        workspace.write("app/other.py", "from independent.renderer import emit as paint\n\ndef page(request):\n    return paint('catalog/card.html', request)\n");
        let mut empty = pack.clone();
        empty["replace"] = true.into();
        empty["semantic_rules"] = serde_json::json!([]);
        write_pack(&empty);
        workspace.delta(&[&path, ".forge/frameworks/another.toml", "app/other.py"]);
        assert_edges("app/views.py", None);
        assert_edges("app/other.py", Some(TARGET));

        workspace.remove_file(&path).unwrap();
        workspace.delta(&[&path]);
        assert_edges("app/views.py", None);
        assert_edges("app/other.py", Some(TARGET));
        write_pack(&pack);
        workspace.delta(&[&path]);
        assert_edges("app/views.py", Some(TARGET));

        // A second namespace/rule with a different actual target makes the
        // occurrence ambiguous even though each individual rule is unique.
        let mut conflicting = pack["semantic_rules"][0].clone();
        conflicting["id"] = "conflicting-file".into();
        conflicting["namespace"] = "other".into();
        conflicting["operation"]["target"]["prefix"] = "other/".into();
        pack["semantic_rules"]
            .as_array_mut()
            .unwrap()
            .push(conflicting);
        workspace.write("app/other/catalog/card.html", "<p>Other</p>\n");
        write_pack(&pack);
        workspace.delta(&[&path, "app/other/catalog/card.html"]);
        assert_edges("app/views.py", None);
        let ambiguous: Vec<_> = workspace
            .observe()
            .coverage()
            .iter()
            .filter(|coverage| {
                coverage.path == "app/views.py"
                    && coverage.expression == "paint('catalog/card.html', request)"
            })
            .map(|coverage| coverage.status.clone())
            .collect();
        assert_eq!(ambiguous, ["ambiguous"]);

        pack["semantic_rules"].as_array_mut().unwrap().pop();
        write_pack(&pack);
        workspace.delta(&[&path]);
        assert_edges("app/views.py", Some(TARGET));

        // A same-name file in a nested project is not a target.
        workspace.write(
            "nested/pyproject.toml",
            "[project]\nname='nested'\ndependencies=['workshop']\n",
        );
        workspace.write("nested/views/catalog/card.html", "<p>Nested</p>\n");
        workspace.delta(&["nested/pyproject.toml", "nested/views/catalog/card.html"]);
        assert_edges("app/views.py", Some(TARGET));

        workspace.remove_file(TARGET).unwrap();
        workspace.delta(&[TARGET]);
        assert_edges("app/views.py", None);
        workspace.write("app/notviews/catalog/card.html", "<p>Wrong segment</p>\n");
        workspace.delta(&["app/notviews/catalog/card.html"]);
        assert_edges("app/views.py", None);
        workspace.write(TARGET, "<p>Restored</p>\n");
        workspace.delta(&[TARGET]);
        assert_edges("app/views.py", Some(TARGET));

        // The canonical resolver must reject a same-spelling local module.
        workspace.write(
            "workshop/renderer.py",
            "def emit(path, request): return path\n",
        );
        workspace.delta(&["workshop/renderer.py"]);
        workspace.assert_cold_parity();
        assert_edges("app/views.py", None);
    }
}
