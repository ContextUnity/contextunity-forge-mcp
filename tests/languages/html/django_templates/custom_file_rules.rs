use super::*;

pub(super) fn custom_file_rules() {
    // The same compiled production pipeline accepts an unregistered pack/API,
    // a different argument index and a different complete directory projection.
    for extension in ["toml", "yaml", "yml"] {
        let workspace = Workspace::new();
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
        write_pack(&pack);
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        if extension != "toml" {
            continue;
        }
        const OVERLAP: &str = "app/views/views/views/catalog/card.html";
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/views/".into();
        workspace.write(OVERLAP, "<p>Overlapping normal segments</p>\n");
        write_pack(&pack);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(&path), PathBuf::from(OVERLAP)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((OVERLAP, 4)));
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        fs::remove_file(workspace.0.join(OVERLAP)).unwrap();
        write_pack(&pack);
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[PathBuf::from(&path), PathBuf::from(OVERLAP)],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        pack["semantic_rules"][0]["namespace"] = "renamed".into();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "missing/".into();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        pack["semantic_rules"][0]["operation"]["target"]["prefix"] = "views/".into();
        pack["semantic_rules"][0]["activation"] =
            serde_json::json!([{"kind":"package","name":"workshop"}]);
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(
            "pyproject.toml",
            "[project]\nname='custom-file-rules'\ndependencies=['workshop']\n",
        );
        workspace.delta("pyproject.toml");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

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
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from(&path),
                PathBuf::from(".forge/frameworks/another.toml"),
                PathBuf::from("app/other.py"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        file_edges(&workspace, "app/other.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(&path)).unwrap();
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        file_edges(&workspace, "app/other.py", Some((TARGET, 4)));
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

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
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from(&path),
                PathBuf::from("app/other/catalog/card.html"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        let ambiguous: String = workspace.open().query_row("SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary p ON p.path_id=rc.path_id JOIN coverage_expressions e ON e.expression_id=rc.expression_id WHERE p.path='app/views.py' AND e.expression=?1", ["paint('catalog/card.html', request)"], |row| row.get(0)).unwrap();
        assert_eq!(ambiguous, "ambiguous");
        pack["semantic_rules"].as_array_mut().unwrap().pop();
        write_pack(&pack);
        workspace.delta(&path);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        // An actual same-name file in another Python project is not a target.
        workspace.write(
            "nested/pyproject.toml",
            "[project]\nname='nested'\ndependencies=['workshop']\n",
        );
        workspace.write("nested/views/catalog/card.html", "<p>Nested</p>\n");
        writer::delta(
            &workspace.0,
            &workspace.db(),
            &[
                PathBuf::from("nested/pyproject.toml"),
                PathBuf::from("nested/views/catalog/card.html"),
            ],
        )
        .unwrap();
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));
        fs::remove_file(workspace.0.join(TARGET)).unwrap();
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write("app/notviews/catalog/card.html", "<p>Wrong segment</p>\n");
        workspace.delta("app/notviews/catalog/card.html");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
        workspace.write(TARGET, "<p>Restored</p>\n");
        workspace.delta(TARGET);
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", Some((TARGET, 4)));

        // The canonical resolver must reject a same-spelling local module.
        workspace.write(
            "workshop/renderer.py",
            "def emit(path, request): return path\n",
        );
        workspace.delta("workshop/renderer.py");
        complete_parity(&workspace);
        file_edges(&workspace, "app/views.py", None);
    }
}
